# 🔄 Repository Moved

This project has been **moved** to a new location:

👉 **[https://github.com/tun-rs/tun-benchmark2](https://github.com/tun-rs/tun-benchmark2)**


# [tun-rs](https://github.com/tun-rs/tun-rs) Performance Test Report

Testing method: Utilize a TUN proxy for traffic.

Data flow path: TcpStream.write -> tun.recv -> tun.send -> TcpStream.read

## Linux

machine: ubuntu 24C/32G

### Asynchronous(Tokio)

| Mode              | Send Speed (MB/s) | Receive Speed (MB/s) |  
|-------------------|-------------------|----------------------|  
| recv/send         | 700.22            | 697.96               |  
| recv/send+channel | 855.59            | 853.03               |  
| TSO               | 719.96            | 718.19               |  
| TSO + Channel     | 1308.58           | 1306.02              |  
| multi-queue       | 669.39            | 667.37               |  

### Synchronous

| Mode      | Send Speed (MB/s) | Receive Speed (MB/s) |  
|-----------|-------------------|----------------------|  
| recv/send | 869.58            | 866.24               |  

- The **TSO + Channel** combination provides the best throughput, achieving approximately **86.8% improvement** over the
  standard mode.
- Using **Channel** alone improves performance by **22.2%** compared to the standard mode.
- **TSO alone** provides a slight improvement, about **2.8%** higher than the standard mode.
- **Multi-queue** performs slightly worse for a single connection.
- **Consider prioritizing the TSO + Channel approach** to achieve optimal performance.

## Windows

machine: Windows11 24C/32G

### Asynchronous(Tokio)

| Mode              | Send Speed (MB/s) | Receive Speed (MB/s) |  
|-------------------|-------------------|----------------------|  
| recv/send         | 333.20            | 329.02               |  
| recv/send+channel | 340.94            | 337.52               |  

### Synchronous

| Mode      | Send Speed (MB/s) | Receive Speed (MB/s) |  
|-----------|-------------------|----------------------|  
| recv/send | 250.34            | 247.19               |  

- Windows is limited by Wintun, resulting in poor performance.
- Asynchronous is superior to synchronous, with a performance improvement of approximately 30%.
