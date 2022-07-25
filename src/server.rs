const REQUEST_BUF_SIZE: usize = 1024;

use std::env;

use crate::args;

use crate::http::*;
use crate::stats::*;

use clap::Parser;
use tokio::net::TcpStream;

use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

use anyhow::Result;

pub fn main() -> Result<()> {
    // Configure logging
    // You can print logs (to stderr) using
    // `log::info!`, `log::warn!`, `log::error!`, etc.
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .init();

    // Parse command line arguments
    let args = args::Args::parse();

    // Set the current working directory
    env::set_current_dir(&args.files)?;

    // Print some info for debugging
    log::info!("HTTP server initializing ---------");
    log::info!("Port:\t\t{}", args.port);
    log::info!("Num threads:\t{}", args.num_threads);
    log::info!("Directory:\t\t{}", &args.files);
    log::info!("----------------------------------");

    // Initialize a thread pool that starts running `listen`
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(args.num_threads)
        .build()?
        .block_on(listen(args.port))
}

async fn listen(port: u16) -> Result<()> {
    // Hint: you should call `handle_socket` in this function.
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    loop {
        let (socket, _) = listener.accept().await?;
        tokio::spawn(async move {
             handle_socket(socket).await
        });
    }
}

// Handle helper
async fn handle_socket_helper(mut socket: TcpStream, mut file: File, path: &str) -> Result<()> {
    // Send header
    let len = file.seek(SeekFrom::End(0)).await?;
    file.seek(SeekFrom::Start(0)).await?;
    let mime = get_mime_type(path);
    start_response(&mut socket, 200).await?;
    send_header(&mut socket, "Content-Type", &mime).await?;
    send_header(&mut socket, "Content-Length", &(len.to_string())).await?;
    end_headers(&mut socket).await?;
    // Send content
    let mut buf = [0; REQUEST_BUF_SIZE];
    let mut read = file.read(&mut buf[..]).await?;
    while read > 0 {
        continue_response(&mut socket, &std::str::from_utf8(&buf[..read]).unwrap().to_string()).await?;
        read = file.read(&mut buf[..]).await?;
    }
    Ok(())
}

// Handles a single connection via `socket`.
async fn handle_socket(mut socket: TcpStream) -> Result<()> {
    let mut request = parse_request(&mut socket).await?;
    let attr = match tokio::fs::metadata(format!(".{}", request.path)).await {
        Ok(attr) => attr,
        Err(e) => {
            start_response(&mut socket, 404).await?;
            return Err(e.into());
        },
    };
    let mut file;
    if attr.is_dir() {
        let mut entries_list = Vec::new();
        let mut len = 0;
        let mut entries = tokio::fs::read_dir(format!(".{}", request.path)).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_name() == "index.html" {
                file = File::open(format!(".{}", format_index(&request.path))).await?;
                handle_socket_helper(socket, file, "index.html").await?;
                return Ok(());
            }
            len += 22 + 2 * (&entry.file_name()).len() + (&request.path).len();
            entries_list.push(entry.file_name());
        }
        // At this point, there is no index.html
        // Parent directory set-up
        let path = std::path::Path::new(&request.path);
        let parent_wrapped = path.parent();
        let parent = if parent_wrapped != None {
            parent_wrapped.unwrap().as_os_str()
        } else {
            path.as_os_str()
        };
        len += 21 + 2 * parent.len();
        // End parent directory set-up
        let mime = get_mime_type("info.html");
        start_response(&mut socket, 200).await?;
        send_header(&mut socket, "Content-Type", &mime).await?;
        send_header(&mut socket, "Content-Length", &(len.to_string())).await?;
        end_headers(&mut socket).await?;
        for entry in &entries_list {
            let entry_str = std::ffi::OsStr::to_str(&entry).unwrap();
            let formatted_href = format_href(&format!("{}/{}", request.path, entry_str), &entry_str);
            continue_response(&mut socket, &formatted_href).await?;
        }
        let parent_str = std::ffi::OsStr::to_str(&parent).unwrap();
        let parent_formatted_href = format_href(parent_str, parent_str);
        continue_response(&mut socket, &parent_formatted_href).await?;
    } else {
        file = File::open(format!(".{}", request.path)).await?;
        handle_socket_helper(socket, file, &request.path).await?;
    }
    Ok(())
}

// You are free (and encouraged) to add other functions to this file.
// You can also create your own modules as you see fit.
