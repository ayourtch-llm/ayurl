/// Integration tests for SCP and SFTP handlers using ayssh's test SSH server.
///
/// These tests spin up a real (in-process) SSH server, then use ayurl's
/// SCP/SFTP handlers to transfer data through it.

use std::sync::Arc;

use ayssh::server::host_key::HostKeyPair;
use ayssh::server::sftp_server::MemoryFs;
use ayssh::server::test_server::TestSshServer;

/// Helper: start a test SSH server on a random port, return the server
/// and its address string suitable for URLs.
async fn start_ssh_server() -> (TestSshServer, String) {
    let host_key = HostKeyPair::generate_ed25519();
    let server = TestSshServer::new(0)
        .await
        .unwrap()
        .with_host_key(host_key);
    let addr = server.local_addr();
    let addr_str = format!("127.0.0.1:{}", addr.port());
    (server, addr_str)
}

// === SCP Download Tests ===

#[tokio::test]
async fn scp_download_small_file() {
    let (server, addr) = start_ssh_server().await;
    let test_data = b"hello from scp server";
    let test_data_clone = test_data.to_vec();

    // Server task: accept connection, send file
    let server_task = tokio::spawn(async move {
        let stream = server.accept_stream().await.unwrap();
        let (mut io, channel) = server.handshake_and_auth(stream).await.unwrap();
        ayssh::server::test_server::handle_scp_download(
            &mut io,
            channel,
            "test.txt",
            &test_data_clone,
            0o644,
        )
        .await
        .unwrap();
    });

    // Client: download via ayurl
    let uri = format!("scp://testuser:testpass@{addr}/test.txt");
    let data = ayurl::get(&uri).await.unwrap().bytes().await.unwrap();

    assert_eq!(data, test_data);
    let _ = server_task.await;
}

#[tokio::test]
async fn scp_download_larger_file() {
    let (server, addr) = start_ssh_server().await;
    // 10KB test file (test server sends as single packet, must fit SSH packet limit)
    let test_data: Vec<u8> = (0..10_000).map(|i| (i % 256) as u8).collect();
    let test_data_clone = test_data.clone();

    let server_task = tokio::spawn(async move {
        let stream = server.accept_stream().await.unwrap();
        let (mut io, channel) = server.handshake_and_auth(stream).await.unwrap();
        ayssh::server::test_server::handle_scp_download(
            &mut io,
            channel,
            "large.bin",
            &test_data_clone,
            0o644,
        )
        .await
        .unwrap();
    });

    let uri = format!("scp://testuser:testpass@{addr}/large.bin");
    let data = ayurl::get(&uri).await.unwrap().bytes().await.unwrap();

    assert_eq!(data.len(), 10_000);
    assert_eq!(data, test_data);
    let _ = server_task.await;
}

#[tokio::test]
async fn scp_download_streaming() {
    use futures::io::AsyncReadExt;

    let (server, addr) = start_ssh_server().await;
    let test_data = b"streaming scp content";
    let test_data_clone = test_data.to_vec();

    let server_task = tokio::spawn(async move {
        let stream = server.accept_stream().await.unwrap();
        let (mut io, channel) = server.handshake_and_auth(stream).await.unwrap();
        ayssh::server::test_server::handle_scp_download(
            &mut io,
            channel,
            "stream.txt",
            &test_data_clone,
            0o644,
        )
        .await
        .unwrap();
    });

    let uri = format!("scp://testuser:testpass@{addr}/stream.txt");
    let mut response = ayurl::get(&uri).await.unwrap();

    // Read via AsyncRead interface
    let mut buf = Vec::new();
    response.read_to_end(&mut buf).await.unwrap();
    assert_eq!(buf, test_data);

    let _ = server_task.await;
}

#[tokio::test]
async fn scp_download_content_length_known() {
    let (server, addr) = start_ssh_server().await;
    let test_data = b"sized content";
    let test_data_clone = test_data.to_vec();

    let server_task = tokio::spawn(async move {
        let stream = server.accept_stream().await.unwrap();
        let (mut io, channel) = server.handshake_and_auth(stream).await.unwrap();
        ayssh::server::test_server::handle_scp_download(
            &mut io,
            channel,
            "sized.txt",
            &test_data_clone,
            0o644,
        )
        .await
        .unwrap();
    });

    let uri = format!("scp://testuser:testpass@{addr}/sized.txt");
    let response = ayurl::get(&uri).await.unwrap();

    // SCP protocol reports file size — should be reflected in content_length
    assert_eq!(response.content_length(), Some(test_data.len() as u64));

    let data = response.bytes().await.unwrap();
    assert_eq!(data, test_data);
    let _ = server_task.await;
}

// === SCP Upload Tests ===

#[tokio::test]
async fn scp_upload_small_file() {
    let (server, addr) = start_ssh_server().await;
    let upload_data = b"uploaded via ayurl";

    let server_task = tokio::spawn(async move {
        let stream = server.accept_stream().await.unwrap();
        let (mut io, channel) = server.handshake_and_auth(stream).await.unwrap();
        let (filename, data) =
            ayssh::server::test_server::handle_scp_upload(&mut io, channel)
                .await
                .unwrap();
        (filename, data)
    });

    let uri = format!("scp://testuser:testpass@{addr}/upload.txt");
    let written = ayurl::put(&uri)
        .bytes(upload_data.to_vec())
        .content_length(upload_data.len() as u64)
        .await
        .unwrap();

    assert_eq!(written, upload_data.len() as u64);

    let (filename, received_data) = server_task.await.unwrap();
    assert_eq!(filename, "upload.txt");
    assert_eq!(received_data, upload_data);
}

// === SFTP Tests ===

#[tokio::test]
async fn sftp_download_file() {
    let (server, addr) = start_ssh_server().await;
    let fs = Arc::new(MemoryFs::new());
    // Store without leading / since our URL parser strips it
    fs.add_file("test.txt", b"sftp file content", 0o644);
    let fs_clone = fs.clone();

    let server_task = tokio::spawn(async move {
        let stream = server.accept_stream().await.unwrap();
        let (mut io, channel) = server.handshake_and_auth(stream).await.unwrap();
        ayssh::server::test_server::run_sftp_server(&mut io, channel, fs_clone)
            .await
            .unwrap();
    });

    let uri = format!("sftp://testuser:testpass@{addr}/test.txt");
    let data = ayurl::get(&uri).await.unwrap().bytes().await.unwrap();

    assert_eq!(data, b"sftp file content");
    let _ = server_task.await;
}

#[tokio::test]
async fn sftp_upload_file() {
    let (server, addr) = start_ssh_server().await;
    let fs = Arc::new(MemoryFs::new());
    let fs_clone = fs.clone();

    let server_task = tokio::spawn(async move {
        let stream = server.accept_stream().await.unwrap();
        let (mut io, channel) = server.handshake_and_auth(stream).await.unwrap();
        ayssh::server::test_server::run_sftp_server(&mut io, channel, fs_clone)
            .await
            .unwrap();
    });

    let uri = format!("sftp://testuser:testpass@{addr}/uploaded.txt");
    let written = ayurl::put(&uri)
        .text("sftp uploaded content")
        .await
        .unwrap();

    assert_eq!(written, 21);

    // Verify the file landed in the in-memory filesystem
    // Try both with and without leading slash
    let stored = fs.get_file("uploaded.txt").or_else(|| fs.get_file("/uploaded.txt"));
    assert_eq!(stored.as_deref(), Some(b"sftp uploaded content".as_slice()));

    let _ = server_task.await;
}

#[tokio::test]
async fn sftp_roundtrip() {
    let (server, addr) = start_ssh_server().await;
    let fs = Arc::new(MemoryFs::new());
    let fs_clone = fs.clone();

    // Server handles multiple connections sequentially (we'll do 2)
    let server_task = tokio::spawn(async move {
        // First connection: upload
        let stream = server.accept_stream().await.unwrap();
        let (mut io, channel) = server.handshake_and_auth(stream).await.unwrap();
        ayssh::server::test_server::run_sftp_server(&mut io, channel, fs_clone.clone())
            .await
            .unwrap();

        // Second connection: download
        let stream = server.accept_stream().await.unwrap();
        let (mut io, channel) = server.handshake_and_auth(stream).await.unwrap();
        ayssh::server::test_server::run_sftp_server(&mut io, channel, fs_clone)
            .await
            .unwrap();
    });

    let base_uri = format!("sftp://testuser:testpass@{addr}");

    // Upload (double-slash for absolute path that includes leading /)
    ayurl::put(&format!("{base_uri}/roundtrip.bin"))
        .bytes(b"roundtrip data".to_vec())
        .await
        .unwrap();

    // Download (same path)
    let data = ayurl::get(&format!("{base_uri}/roundtrip.bin"))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    assert_eq!(data, b"roundtrip data");
    let _ = server_task.await;
}

// === SCP with progress ===

#[tokio::test]
async fn scp_download_with_progress() {
    use std::sync::atomic::{AtomicU64, Ordering};

    let (server, addr) = start_ssh_server().await;
    let test_data = vec![0xABu8; 5_000];
    let test_data_clone = test_data.clone();

    let server_task = tokio::spawn(async move {
        let stream = server.accept_stream().await.unwrap();
        let (mut io, channel) = server.handshake_and_auth(stream).await.unwrap();
        ayssh::server::test_server::handle_scp_download(
            &mut io,
            channel,
            "progress.bin",
            &test_data_clone,
            0o644,
        )
        .await
        .unwrap();
    });

    let last_bytes = Arc::new(AtomicU64::new(0));
    let last_bytes_clone = last_bytes.clone();

    let uri = format!("scp://testuser:testpass@{addr}/progress.bin");
    let data = ayurl::get(&uri)
        .on_progress(move |p| {
            last_bytes_clone.store(p.bytes_transferred, Ordering::Relaxed);
        })
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    assert_eq!(data.len(), 5_000);
    assert_eq!(last_bytes.load(Ordering::Relaxed), 5_000);
    let _ = server_task.await;
}
