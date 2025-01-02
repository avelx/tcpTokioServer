use std::error::Error;
use std::io;
use std::str;
use crossbeam_channel::{bounded, unbounded, Receiver, SendError, Sender, TryRecvError};
use std::{thread, time};
use std::ops::Add;
use tokio::io::{AsyncReadExt, AsyncWriteExt, Interest};
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use log::{info, warn, error};
use std::net::{Shutdown};
use crossbeam_channel::internal::SelectHandle;

#[tokio::main]
async fn main() -> io::Result<()> {

    const REMOTE_RESOURCE: &str = "216.58.204.78:80";

    //colog::init();

    let mut builder = colog::default_builder();
    builder.filter(None, log::LevelFilter::Trace);
    builder.init();

    //let mut clog = colog::default_builder();
    //clog.filter(None, log::LevelFilter::Warn);
    //clog.filter(None, log::LevelFilter::Info);
    //clog.init();

    async fn remote_server_thread(
        txA: Sender<String>,
        rxA: Receiver<String>,
    ) -> Result<(), Box<dyn Error>> {
        info!("Setup remote connection thread ...");

        let std_stream = std::net::TcpStream::connect(REMOTE_RESOURCE)?;
        std_stream.set_nonblocking(true)?;
        let mut remoteStream = TcpStream::from_std(std_stream).unwrap();
        loop {
            let ready = remoteStream
                .ready(Interest::READABLE | Interest::WRITABLE)
                .await
                .unwrap();

            info!("Remote->Stream status: {:?}", ready);
            //let ten_millis = time::Duration::from_millis(2000);
            //thread::sleep(ten_millis);

            if ready.is_readable() {
                info!("Remote->Ready to read ...");
                let mut data = vec![0; 8024];
                // Try to read data, this may still fail with `WouldBlock`
                // if the readiness event is a false positive.
                match remoteStream.try_read(&mut data) {
                    Ok(n) => {
                        let response_string = str::from_utf8(&data).unwrap();
                        info!("Remote->Response2:\n\r{}", response_string);
                        //let val = String::from("hi");
                        txA.send(response_string.to_string()).unwrap();
                        return Ok(());
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        error!("Remote->Read2: {}", e);
                        continue;
                    }
                    Err(e) => {
                        error!("Remote->Read2: {}", e);
                        return Err(e.into());
                    }
                }
            }


            if ready.is_writable() {
                info!("Writable ...");
                for request_remote in rxA.try_recv() {
                    let request_remote2: String = request_remote
                        .replace("Host: localhost:8181", "Host: google.com");
                        //.replace("GET /", "GET http://google.com");
                        //.add("\r\n");
                        //.trim().to_string();
                    info!("Request->Remote/modified:\n\r{}", request_remote2);

                    // Try to write data, this may still fail with `WouldBlock`
                    // if the readiness event is a false positive.
                    if request_remote2.len() > 0 {
                        info!("Attempt to write");
                        match remoteStream.try_write(request_remote2.as_bytes()) {
                            Ok(n) => {
                                info!("Remote write {} bytes", n);
                                //remoteStream.flush().await.unwrap();
                                //remoteStream.shutdown(); //.expect("shutdown call failed")

                                let mut data = vec![0; 8024];
                                // Try to read data, this may still fail with `WouldBlock`
                                // if the readiness event is a false positive.
                                let mut response: String = String::from("");
                                remoteStream.read_to_string(&mut response).await.unwrap();
                                //let response_string = str::from_utf8(&data).unwrap();
                                info!("Remote->Response:\n\r{} - {}", response, response.len());
                                if response.len() > 0 {
                                    let res: Result<(), SendError<String>> = txA.send(response.to_string());
                                        //txA.send(response.to_string()).unwrap_err();
                                    match res {
                                        Ok(e) => {
                                            error!("Error sending response: {:?}", e);
                                            continue;
                                        }
                                        // SendError(e: String) => {
                                        //     error!("Error sending response: {:?}", e);
                                        //     continue;
                                        // },
                                        _ => {
                                            error!("Error sending response v6");
                                            continue
                                        }
                                    }
                                }

                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                error!("Remote write error: {}", e);
                                continue
                            }
                            Err(e) => {
                                error!("Remote write error: {}", e);
                                return Err(e.into());
                            }
                        }
                    }
                }
            }
            //continue;
        }
    }

    async fn process_local_stream(local_stream: TcpStream) -> Result<(), Box<dyn Error>> {

        //Transmitter from local / Receiver to remote
        let (txLocal, rxRemote) = bounded(0);
        // Transmitter from remote, Receiver to local
        let (txRemote, rxLocal) = bounded(0);

         tokio::spawn(async {
            remote_server_thread(txRemote, rxRemote).await;
        });
        // let handler = thread::spawn(move || {remote_server_thread(txRemote, rxRemote) } );
        // handler.join().unwrap();

        info!("Local:Processing ...");
        loop {
            let ready = local_stream
                .ready(Interest::READABLE | Interest::WRITABLE)
                .await
                .unwrap();

            info!("Local->Stream status: {:?}", ready);

            if ready.is_readable() {
                let mut data = vec![0; 1024];
                // Try to read data, this may still fail with `WouldBlock`
                // if the readiness event is a false positive.
                match local_stream.try_read(&mut data) {
                    Ok(n) => {
                        let response_string = str::from_utf8(&data).unwrap();
                        info!("Local->Response: {}", response_string);
                        //let val = String::from("hi");
                        txLocal.send(response_string.to_string()).unwrap();
                        continue;
                        //return Ok(());
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        error!("Local->Read error: {}", e);
                        continue;
                    }
                    Err(e) => {
                        error!("Local->Read error/v2: {}", e);
                        return Err(e.into());
                    }
                }
            }

            if ready.is_writable() {
                info!("Local->Read ...");
                let request_local: Result<String, TryRecvError> = rxLocal.try_recv();
                //info!("Local->Request bytes: {}", request_local);
                // Try to write data, this may still fail with `WouldBlock`
                // if the readiness event is a false positive.
                match request_local {
                    Ok(request) => {
                        match local_stream.try_write(request.as_ref()) {
                            Ok(n) => {
                                info!("Local=>write {} bytes", n);
                                continue
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                error!("Local->Write error: {}", e);
                                continue
                            },
                            Err(e) => {
                                error!("Local->Write error/v2: {}", e);
                                return Err(e.into());
                            }
                        }
                    }
                    Err(e) => {
                        error!("Local->Write error/v3: {}", e);
                        continue;
                    }
                }
                // if request_local.len() > 0 {
                //
                // }
            }
        }


    }

    let local_listener = TcpListener::bind("127.0.0.1:8181").await?;
    loop {
        let (socket, _) = local_listener.accept().await?;
        print!("Ready to accept connection ...");
        process_local_stream(socket).await.expect("Filed to process incoming connection");
    }

}
