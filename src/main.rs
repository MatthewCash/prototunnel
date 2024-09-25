use anyhow::{bail, Result};
use clap::Parser;
use ipnet::Ipv4Net;
use log::{debug, error, info};
use std::{net::SocketAddr, os::fd::AsRawFd, pin::Pin};
use tokio::{
    io::{split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    task, try_join,
};
use tokio_tun::Tun;
use udp_stream::UdpStream;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    address: Ipv4Net,

    #[arg(short, long, default_value = "prototun")]
    name: String,

    #[arg(short, long, conflicts_with = "client")]
    server: Option<SocketAddr>,

    #[arg(short, long, conflicts_with = "server")]
    client: Option<SocketAddr>,

    #[arg(short, long, default_value_t = false)]
    tcp: bool,

    #[arg(short, long, default_value_t = 1500)]
    mtu: u16,
}

pub trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite {}

async fn pipe(
    mut reader: Box<dyn AsyncRead + Unpin + Send>,
    mut writer: Box<dyn AsyncWrite + Unpin + Send>,
    mtu: u16,
) -> Result<()> {
    let mut buf = vec![0u8; mtu as usize + 4]; // 4 bytes for packet info

    loop {
        let bytes_read = reader.read(&mut buf).await?;
        let bytes_sent = writer.write(&buf[..bytes_read]).await?;
        if bytes_read != bytes_sent {
            bail!("Read {bytes_read} bytes but sent {bytes_sent} bytes!");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let args = Args::parse();

    let tun = Tun::builder()
        .name(&args.name)
        .packet_info()
        .address(args.address.addr())
        .netmask(args.address.netmask())
        .mtu(args.mtu as i32)
        .up()
        .try_build()?;

    debug!("tun ({}) created with fd {}", tun.name(), tun.as_raw_fd());

    let sock_stream: Pin<Box<dyn AsyncReadWrite + Send>> = if let Some(host) = args.server {
        if args.tcp {
            Box::pin(TcpListener::bind(host).await?.accept().await?.0)
        } else {
            let socket = UdpSocket::bind(host).await?;
            let (_, addr) = socket.recv_from(&mut []).await?;
            Box::pin(UdpStream::from_tokio(socket, addr).await?)
        }
    } else if let Some(host) = args.client {
        if args.tcp {
            Box::pin(TcpStream::connect(host).await?)
        } else {
            Box::pin(UdpStream::connect(host).await?)
        }
    } else {
        bail!("Either client or server operation must be specified!");
    };

    info!("Transport stream established!");

    let (sock_reader, sock_writer) = split(sock_stream);
    let (tun_reader, tun_writer) = split(tun);

    let tasks = try_join!(
        task::spawn(pipe(Box::new(sock_reader), Box::new(tun_writer), args.mtu)),
        task::spawn(pipe(Box::new(tun_reader), Box::new(sock_writer), args.mtu))
    )?;

    if let Err(why) = tasks.0 {
        error!("{:?}", why.context("Failed to read from socket to tun!"));
    }

    if let Err(why) = tasks.1 {
        error!("{:?}", why.context("Failed to read from tun to socket!"));
    }

    Ok(())
}
