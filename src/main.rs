use tokio::io;
use tokio::net;
use web_server_tokio::handle_stream;

/// `main` creates a TCP listener, spawns a task for each incoming connection, and awaits for all tasks
/// to complete
///
/// # Errors
///
/// Captures errors from binding to address `127.0.0.1:7878`. Writes errors from accepting stream
/// or handling connections to stderr.
#[tokio::main]
async fn main() -> io::Result<()> {
    let listener = net::TcpListener::bind("127.0.0.1:7878").await?;
    let capacity = 10;
    let mut tasks = Vec::with_capacity(capacity);

    for count in 1..=capacity {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(error) => {
                dbg!(error);
                continue;
            }
        };
        let task = tokio::spawn(async move {
            match handle_stream(Box::new(stream)).await {
                Ok(()) => {
                    println!("Completed request {}.", count);
                }
                Err(error) => {
                    dbg!(error);
                }
            }
        });
        tasks.push(task);
    }

    // Without awaiting for tasks, main thread will exit before slowest task completes
    for task in tasks {
        let _ = task.await;
    }
    Ok(())
}
