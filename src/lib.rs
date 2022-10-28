use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::{fs, io, net, time};

/// Enables [`handle_stream`] to work with [`net::TcpStream`] for release
/// and mock struct implementations for testing.
#[async_trait]
pub trait StreamAdapter: Send {
    /// Reads the first line of the request.
    ///
    /// # Returns
    ///
    /// A string of the first line of the request.
    async fn read_request(&mut self) -> io::Result<String>;

    /// Writes the response to the client.
    ///
    /// # Arguments
    ///
    /// * `response`: The response to write to the client.
    ///
    /// # Returns
    ///
    /// The result of the write_all function.
    async fn write_response(&mut self, response: &[u8]) -> io::Result<()>;
}

/// Implementing the [`StreamAdapter`] trait for the [`net::TcpStream`] struct.
#[async_trait]
impl StreamAdapter for net::TcpStream {
    /// Reads the first line of the request.
    ///
    /// # Returns
    ///
    /// A string of the first line of the request.
    async fn read_request(&mut self) -> io::Result<String> {
        Ok(io::BufReader::new(&mut self)
            .lines()
            .next_line()
            .await?
            .unwrap_or_default())
    }

    /// Writes the response to the client.
    ///
    /// # Arguments
    ///
    /// * `response`: The response to write to the client.
    ///
    /// # Returns
    ///
    /// The result of the write_all function.
    async fn write_response(&mut self, response: &[u8]) -> io::Result<()> {
        Ok(self.write_all(response).await?)
    }
}

/// It reads a request from the stream, then it either returns a 200 OK response with the contents of
/// `hello.html` or a 404 NOT FOUND response with the contents of `404.html`.
/// Coupled to [`StreamAdapter`] to enable test doubles.
///
/// # Arguments
///
/// * `stream`: An incoming stream.
///
/// # Returns
///
/// Returns either Ok(()) or a propagated IO error.
///
/// # Errors
///
/// Captures IO errors from any of the following:
/// * Reading request line from stream
/// * Reading contents for response from a file
/// * Writing response to stream
pub async fn handle_stream(mut stream: Box<dyn StreamAdapter>) -> io::Result<()> {
    let request = stream.read_request().await?;
    let (status_line, file_name) = match request.as_str() {
        "GET / HTTP/1.1" => ("HTTP/1.1 200 OK", "hello.html"),
        "GET /sleep HTTP/1.1" => {
            time::sleep(time::Duration::from_secs(5)).await;
            ("HTTP/1.1 200 OK", "hello.html")
        }
        _ => ("HTTP/1.1 404 NOT FOUND", "404.html"),
    };
    let contents = fs::read_to_string(file_name).await?;
    let response = format!(
        "{}\r\nContent-Length: {}\r\n\r\n{}",
        status_line,
        contents.len(),
        contents
    );
    stream.write_response(response.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time;

    const HELLO_HTML: &str = "\
<!DOCTYPE html>\r
<html lang=\"en\">\r
<head>\r
    <meta charset=\"utf-8\">\r
    <title>Hello!</title>\r
</head>\r
<body>\r
<h1>Hello!</h1>\r
<p>Hi from Rust</p>\r
</body>\r
</html>";
    const FOUR04_HTML: &str = "\
<!DOCTYPE html>\r
<html lang=\"en\">\r
<head>\r
    <meta charset=\"utf-8\">\r
    <title>Hello!</title>\r
</head>\r
<body>\r
<h1>Oops!</h1>\r
<p>Sorry, I don't know what you're asking for.</p>\r
</body>\r
</html>";

    struct NoErrorMockStream {
        request: &'static str,
        expected_response: String,
    }

    /// Implementing the [`StreamAdapter`] trait for the [`NoErrorMockStream`] struct.
    #[async_trait]
    impl StreamAdapter for NoErrorMockStream {
        async fn read_request(&mut self) -> io::Result<String> {
            Ok(self.request.to_string())
        }

        async fn write_response(&mut self, response: &[u8]) -> io::Result<()> {
            assert_eq!(self.expected_response.as_bytes(), response);
            Ok(())
        }
    }

    enum ErrorLocation {
        Request,
        Response,
    }

    struct ErrorMockStream {
        error_location: ErrorLocation,
        error_kind: io::ErrorKind,
    }

    /// Implementing the [`StreamAdapter`] trait for the [`ErrorMockStream`] struct.
    #[async_trait]
    impl StreamAdapter for ErrorMockStream {
        async fn read_request(&mut self) -> io::Result<String> {
            match self.error_location {
                ErrorLocation::Request => Err(io::Error::from(self.error_kind)),
                ErrorLocation::Response => Ok("GET / HTTP/1.1".to_string()),
            }
        }

        async fn write_response(&mut self, _response: &[u8]) -> io::Result<()> {
            match self.error_location {
                ErrorLocation::Response => Err(io::Error::from(self.error_kind)),
                ErrorLocation::Request => Ok(()),
            }
        }
    }

    /// It creates a mock stream, passes it to the `handle_stream` function, and asserts that the result
    /// is `Ok(())`
    #[tokio::test]
    async fn get_immediately() {
        let mock_stream = NoErrorMockStream {
            request: "GET / HTTP/1.1",
            expected_response: format!(
                "{}\r\nContent-Length: {}\r\n\r\n{}",
                "HTTP/1.1 200 OK",
                HELLO_HTML.len(),
                HELLO_HTML
            ),
        };
        let ok = handle_stream(Box::new(mock_stream)).await.unwrap();
        assert_eq!((), ok);
    }

    /// It creates a mock stream that sends a request for `/sleep` and expects a response with the
    /// contents of `HELLO_HTML`
    #[ignore]
    #[tokio::test]
    async fn get_later() {
        let mock_stream = NoErrorMockStream {
            request: "GET /sleep HTTP/1.1",
            expected_response: format!(
                "{}\r\nContent-Length: {}\r\n\r\n{}",
                "HTTP/1.1 200 OK",
                HELLO_HTML.len(),
                HELLO_HTML
            ),
        };
        let minimum_instant = time::Instant::now() + time::Duration::from_secs(5);
        let ok = handle_stream(Box::new(mock_stream)).await.unwrap();
        let now = time::Instant::now();
        assert!(now >= minimum_instant);
        assert_eq!((), ok);
    }

    /// It creates a mock stream, passes it to the `handle_stream` function, and asserts that the result
    /// is `Ok(())`
    #[tokio::test]
    async fn not_found() {
        let mock_stream = NoErrorMockStream {
            request: "",
            expected_response: format!(
                "{}\r\nContent-Length: {}\r\n\r\n{}",
                "HTTP/1.1 404 NOT FOUND",
                FOUR04_HTML.len(),
                FOUR04_HTML
            ),
        };
        let ok = handle_stream(Box::new(mock_stream)).await.unwrap();
        assert_eq!((), ok);
    }

    /// It creates a mock stream, passes it to the `handle_stream` function, and asserts that the result
    /// is `io::ErrorKind::NotFound`
    #[tokio::test]
    async fn invalid_request() {
        let kind = io::ErrorKind::NotFound;
        let mock_stream = ErrorMockStream {
            error_location: ErrorLocation::Request,
            error_kind: kind.clone(),
        };
        let error = handle_stream(Box::new(mock_stream)).await.unwrap_err();
        assert_eq!(kind, error.kind());
    }

    /// It creates a mock stream, passes it to the `handle_stream` function, and asserts that the result
    /// is `io::ErrorKind::NotFound`
    #[tokio::test]
    async fn invalid_response() {
        let kind = io::ErrorKind::NotFound;
        let mock_stream = ErrorMockStream {
            error_location: ErrorLocation::Response,
            error_kind: kind.clone(),
        };
        let error = handle_stream(Box::new(mock_stream)).await.unwrap_err();
        assert_eq!(kind, error.kind());
    }
}
