#!/bin/bash

# Principal Systems Engineer - High-Performance Tuning Script
# Optimization for 32-core high-throughput Ntex/io_uring server

if [[ $EUID -ne 0 ]]; then
   echo "This script must be run as root"
   exit 1
fi

echo "--- Applying Linux Kernel Tunables for Ultra-High RPS ---"

# 1. File Descriptor Limits
# Increase total max open files
sysctl -w fs.file-max=2097152
# Increase per-process limit in limits.conf (requires logout/login or ulimit)
ulimit -n 1048576

# 2. Network Stack - General
# Max number of packets backlogged in the interface queue
sysctl -w net.core.netdev_max_backlog=65535
# Max number of pending connections (listen backlog)
sysctl -w net.core.somaxconn=65535
# Optimization for high-speed handshakes
sysctl -w net.ipv4.tcp_max_syn_backlog=65535
sysctl -w net.ipv4.tcp_slow_start_after_idle=0

# 3. Port Range & TCP Reuse
# Use a wide range for local ports to avoid exhaustion
sysctl -w net.ipv4.ip_local_port_range="1024 65535"
# Fast recycling of TIME_WAIT sockets
sysctl -w net.ipv4.tcp_tw_reuse=1
# Reduce TIME_WAIT timeout
sysctl -w net.ipv4.tcp_fin_timeout=15

# 4. Memory & Buffers (Tuned for 40GB RAM)
# Increase TCP buffer sizes for high-bandwidth/high-concurrency
sysctl -w net.core.rmem_max=16777216
sysctl -w net.core.wmem_max=16777216
sysctl -w net.ipv4.tcp_rmem="4096 87380 16777216"
sysctl -w net.ipv4.tcp_wmem="4096 65536 16777216"
# Increase max number of TCP sockets
sysctl -w net.ipv4.tcp_max_orphans=262144

# 5. Virtual Memory Tuning
# Reduce swapping likelihood
sysctl -w vm.swappiness=10
# Increase number of allowed memory map areas
sysctl -w vm.max_map_count=262144

echo "--- Tuning Complete ---"
echo "Recommended: Set CPU governor to performance"
echo "  for i in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do echo performance > \$i; done"
