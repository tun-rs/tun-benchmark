#![allow(dead_code)]
use std::net::Ipv4Addr;
use std::sync::Arc;

use crate::*;
use bytes::BytesMut;
use pnet_packet::ip::IpNextHeaderProtocols;
use pnet_packet::ipv4::MutableIpv4Packet;
use pnet_packet::tcp::MutableTcpPacket;
use pnet_packet::MutablePacket;
use tun::{AsyncDevice, Configuration, Device};

pub fn start_sync() {
    let mut config = Configuration::default();
    config
        .address(SRC_IP)
        .netmask("255.255.255.0")
        .up()
        .mtu(1500);
    let device = Device::new(&config).unwrap();
    let mut buf = vec![0; 1500];
    loop {
        let len = match device.recv(&mut buf) {
            Ok(len) => len,
            Err(e) => {
                log::error!("device recv {e:?}");
                continue;
            }
        };
        if proxy(&mut buf[..len]).is_some() {
            if let Err(e) = device.send(&buf[..len]) {
                log::error!("device send {e:?}");
            }
        }
    }
}

pub async fn start_async() {
    let mut config = Configuration::default();
    config
        .address(SRC_IP)
        .netmask("255.255.255.0")
        .up()
        .mtu(1500);
    let device = Device::new(&config).unwrap();
    let async_device = AsyncDevice::new(device).unwrap();
    task(async_device).await;
}
pub async fn start_channel_async() {
    let mut config = Configuration::default();
    config
        .address(SRC_IP)
        .netmask("255.255.255.0")
        .up()
        .mtu(1500);
    let device = Device::new(&config).unwrap();
    let async_device = AsyncDevice::new(device).unwrap();
    let async_device = Arc::new(async_device);
    let async_device_send = async_device.clone();
    let mut buf = vec![0; 1500];
    let (s, mut r) = tokio::sync::mpsc::channel::<BytesMut>(128);
    tokio::spawn(async move {
        while let Some(buf) = r.recv().await {
            if let Err(e) = async_device_send.send(&buf).await {
                log::error!("async_device send {e:?}");
            }
        }
    });
    loop {
        let len = match async_device.recv(&mut buf).await {
            Ok(len) => len,
            Err(e) => {
                log::error!("async_device recv {e:?}");
                continue;
            }
        };
        if proxy(&mut buf[..len]).is_some() {
            let mut send_buf = BytesMut::with_capacity(1500);
            send_buf.extend_from_slice(&buf[..len]);
            s.send(send_buf).await.unwrap();
        }
    }
}

async fn task(async_device: AsyncDevice) {
    let mut buf = vec![0; 1500];
    loop {
        let len = match async_device.recv(&mut buf).await {
            Ok(len) => len,
            Err(e) => {
                log::error!("async_device recv {e:?}");
                continue;
            }
        };
        if proxy(&mut buf[..len]).is_some() {
            if let Err(e) = async_device.send(&buf[..len]).await {
                log::error!("async_device send {e:?}");
            }
        }
    }
}

fn proxy(buf: &mut [u8]) -> Option<()> {
    let mut ip_packet = MutableIpv4Packet::new(buf)?;
    if ip_packet.get_source() != SRC_IP {
        return None;
    }
    if ip_packet.get_next_level_protocol() != IpNextHeaderProtocols::Tcp {
        return None;
    }
    if ip_packet.get_destination() == DST_IP_A {
        return proxy_dst(&mut ip_packet, DST_IP_B);
    }
    if ip_packet.get_destination() == DST_IP_B {
        return proxy_dst(&mut ip_packet, DST_IP_A);
    }
    None
}
fn proxy_dst(ip_packet: &mut MutableIpv4Packet, dst: Ipv4Addr) -> Option<()> {
    let mut tcp_packet = MutableTcpPacket::new(ip_packet.payload_mut())?;

    let check_sum = pnet_packet::tcp::ipv4_checksum(&tcp_packet.to_immutable(), &dst, &SRC_IP);

    tcp_packet.set_checksum(check_sum);

    ip_packet.set_destination(SRC_IP);
    ip_packet.set_source(dst);
    let check_sum = pnet_packet::ipv4::checksum(&ip_packet.to_immutable());
    ip_packet.set_checksum(check_sum);
    Some(())
}
