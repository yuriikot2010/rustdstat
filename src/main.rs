use ntex::web::{self, App, HttpResponse, HttpServer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use log::info;

/// Shard with cache-line padding to prevent false sharing.
#[repr(align(64))]
struct Shard {
    count: AtomicU64,
    latency_sum_ns: AtomicU64,
    sampled_count: AtomicU64,
}

impl Shard {
    fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            latency_sum_ns: AtomicU64::new(0),
            sampled_count: AtomicU64::new(0),
        }
    }
}

struct Metrics {
    shards: Vec<Arc<Shard>>,
}

impl Metrics {
    fn new(num_shards: usize) -> Self {
        let mut shards = Vec::with_capacity(num_shards);
        for _ in 0..num_shards {
            shards.push(Arc::new(Shard::new()));
        }
        Metrics { shards }
    }

    fn aggregate(&self) -> (u64, u64, u64) {
        let mut total_count = 0;
        let mut total_latency = 0;
        let mut total_sampled = 0;
        for shard in &self.shards {
            total_count += shard.count.load(Ordering::Relaxed);
            total_latency += shard.latency_sum_ns.load(Ordering::Relaxed);
            total_sampled += shard.sampled_count.load(Ordering::Relaxed);
        }
        (total_count, total_latency, total_sampled)
    }
}

async fn index(shard: web::types::State<Arc<Shard>>) -> HttpResponse {
    thread_local! {
        static REQ_COUNT: std::cell::Cell<u64> = std::cell::Cell::new(0);
    }
    
    let count = REQ_COUNT.with(|c| {
        let val = c.get();
        c.set(val + 1);
        val
    });

    // The Fast Path (127/128 of the time)
    if count & 127 != 0 {
        shard.count.fetch_add(1, Ordering::Relaxed);
        return HttpResponse::NoContent().finish(); 
    }

    // The Sampled Path (1/128 of the time)
    let start = Instant::now();
    let res = HttpResponse::NoContent().finish();
    let elapsed = start.elapsed().as_nanos() as u64;
    
    shard.latency_sum_ns.fetch_add(elapsed, Ordering::Relaxed);
    shard.sampled_count.fetch_add(1, Ordering::Relaxed);
    shard.count.fetch_add(1, Ordering::Relaxed);
    
    res
}

#[ntex::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let num_workers = num_cpus::get();
    let metrics = Arc::new(Metrics::new(num_workers));
    let metrics_for_telemetry = metrics.clone();
    
    let worker_counter = Arc::new(AtomicU64::new(0));

    // Telemetry Task (Aggregator)
    ntex::rt::spawn(async move {
        let mut last_count = 0;
        let mut last_check = Instant::now();
        
        loop {
            ntex::time::sleep(Duration::from_millis(1000)).await;
            
            let (total_count, total_latency_ns, total_sampled) = metrics_for_telemetry.aggregate();
            let now = Instant::now();
            let elapsed = now.duration_since(last_check).as_secs_f64();
            
            let delta_count = total_count.saturating_sub(last_count);
            let rps = delta_count as f64 / elapsed;
            
            let avg_latency_us = if total_sampled > 0 {
                (total_latency_ns as f64 / total_sampled as f64) / 1000.0
            } else {
                0.0
            };

            info!(
                "dstat: | RPS: {:>10.2} | Total: {:>12} | Latency: {:>8.2}us | Workers: {}",
                rps, total_count, avg_latency_us, num_workers
            );
            
            last_count = total_count;
            last_check = now;
        }
    });

    info!("Starting ultra-high performance Ntex server ({} workers)", num_workers);
    
    #[cfg(target_os = "linux")]
    info!("Runtime optimization: io_uring (neon) enabled via features");

    let server = HttpServer::new(move || {
        let worker_idx = worker_counter.fetch_add(1, Ordering::SeqCst) as usize % num_workers;
        let shard = metrics.shards[worker_idx].clone();
        
        App::new()
            .state(shard)
            .service(web::resource("/").to(index))
    })
    .workers(num_workers)
    .backlog(65535)
    .keep_alive(ntex::time::Seconds(300))
    .client_timeout(ntex::time::Seconds(10));

    info!("Binding to 0.0.0.0:8080");
    server.bind("0.0.0.0:8080")?.run().await
}
