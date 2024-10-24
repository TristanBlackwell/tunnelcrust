use core::str;
use std::io::{Cursor, Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use http_body_util::BodyExt;
use hyper::{
    body::{Bytes, Incoming},
    header::{HeaderName, HeaderValue},
    HeaderMap, Method, Request, Uri,
};

/// Serialize / Deserialize signatures for encoding a [`hyper::Method`]
/// in a byte representation.
trait MethodBinaryProtocol {
    fn serialize(&self) -> Result<u8, Box<dyn std::error::Error>>;
    fn deserialize(code: u8) -> Result<Self, Box<dyn std::error::Error>>
    where
        Self: Sized;
}

impl MethodBinaryProtocol for Method {
    /// Serializes a method as a simple u8 integer. This supports all common
    /// methods (including PATCH) but not extension methods.
    ///
    /// This method will return an error if a non-standard method is met.
    fn serialize(&self) -> Result<u8, Box<dyn std::error::Error>> {
        match *self {
            Method::GET => Ok(0),
            Method::POST => Ok(1),
            Method::PUT => Ok(2),
            Method::DELETE => Ok(3),
            Method::PATCH => Ok(4),
            Method::OPTIONS => Ok(5),
            Method::HEAD => Ok(6),
            Method::TRACE => Ok(7),
            Method::CONNECT => Ok(8),
            _ => Err("Unknown method".into()),
        }
    }

    /// Deserializes a method encoded by `serialize()` back into it's
    /// method representation.
    ///
    /// If a matching method cannot be identified then an error is returned.
    fn deserialize(code: u8) -> Result<Self, Box<dyn std::error::Error>> {
        match code {
            0 => Ok(Method::GET),
            1 => Ok(Method::POST),
            2 => Ok(Method::PUT),
            3 => Ok(Method::DELETE),
            4 => Ok(Method::PATCH),
            5 => Ok(Method::OPTIONS),
            6 => Ok(Method::HEAD),
            7 => Ok(Method::TRACE),
            8 => Ok(Method::CONNECT),
            _ => Err("Unknown method.".into()),
        }
    }
}

/// Serialize / Deserialize signatures for encoding [`hyper`]
/// request components in a byte representation.
trait ComponentBinaryProtocol {
    fn serialize(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>>;

    fn deserialize(buffer: &[u8]) -> Result<Self, Box<dyn std::error::Error>>
    where
        Self: std::marker::Sized;
}

impl ComponentBinaryProtocol for hyper::Uri {
    /// Serializes a  [`hyper::Uri`]` in a byte representation. The structure of the returned buffer is 2 bytes
    /// (in big endian encoding) describing the URI length and N bytes for the URI content where
    /// N is the length serialized. You can use the accompanying `deserialize()` method to read this.
    ///
    /// Since 2 bytes are reserved for the URI length the maximum URI length cannot exceed 65535
    /// bytes. This is true of the [`hyper::Uri`] struct anyway so an error in this scenario should
    /// be encountered prior.
    ///
    /// This method will return an Error result if the bytes cannot be written to the buffer
    /// as per the Vec `write_all()` method.
    fn serialize(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut buffer = Vec::new();

        let uri_string = self.to_string();
        let uri_bytes = uri_string.as_bytes();
        let uri_byte_len = uri_bytes.len() as u16; // u16 enforced by `hyper::Uri`

        // Write the length of the URI using 2 bytes (big endian)
        buffer.write_u16::<BigEndian>(uri_byte_len)?;

        buffer.write_all(uri_bytes)?;

        Ok(buffer)
    }

    /// Deserializes a buffer into a [`hyper::Uri`]` representation created from the `serialize()` function.
    /// See `serialize()` for details on the byte representation.
    ///
    /// This method may return an Error in a few cases:
    /// - It is not possible to read the URI length in its u16 representation (big endian).
    /// - The URI (determined by the URI length) cannot be fully read or is interrupted.
    /// - The bytes cannot be read as valid UTF8.
    fn deserialize(buffer: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut cursor = Cursor::new(buffer);

        let uri_len = cursor.read_u16::<BigEndian>()?;

        // Creates a vec of the expected size of our URI bytes
        let mut uri_bytes = vec![0u8; uri_len as usize];
        cursor.read_exact(&mut uri_bytes)?;

        let uri = str::from_utf8(&uri_bytes)?;
        let uri = uri.parse::<hyper::Uri>().unwrap();

        Ok(uri)
    }
}

impl ComponentBinaryProtocol for HeaderMap<HeaderValue> {
    /// Serializes a [`hyper::HeaderMap<HeaderValue>`] into a byte representation. The structure of the returned
    /// buffer is:
    /// - 2 bytes (in big endian encoding) for the number of headers (N).
    /// - X bytes for header content.
    ///
    /// **Each** header is represented by:
    /// - 2 bytes (big endian) for the length of the name.
    /// - Y bytes for the name itself
    ///
    /// - 2 bytes (big endian) for the length of the value.
    /// - Z bytes for the value.
    ///
    /// Therefore the size of X is ((2 + Y) + (2 + Z) * N)
    ///
    /// Since 2 byte chunks are reserved for header count, name, and value any of these
    /// cannot exceed `65535`. An Error will be returned if so.
    ///
    /// This method will also return an Error result if the bytes cannot be written to the buffer
    /// as per the Vec `write_all()` method.
    fn serialize(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut buffer = Vec::new();

        let no_of_headers = self.len();
        if no_of_headers > u16::MAX as usize {
            return Err("Number of headers exceeds maximum u16 representation of 65535".into());
        }
        buffer.write_u16::<BigEndian>(no_of_headers as u16)?;

        for (k, v) in self.iter() {
            let key_bytes = k.as_str().as_bytes();
            let key_bytes_len = key_bytes.len();
            if key_bytes_len > u16::MAX as usize {
                return Err("Key exceeds maximum u16 representation of 65535".into());
            }

            let value_bytes = v.as_bytes();
            let value_bytes_len = value_bytes.len();
            if value_bytes_len > u16::MAX as usize {
                return Err("Value exceeds maximum u16 representation of 65535".into());
            }

            buffer.write_u16::<BigEndian>(key_bytes_len as u16)?;
            buffer.write_all(key_bytes)?;

            buffer.write_u16::<BigEndian>(value_bytes_len as u16)?;
            buffer.write_all(value_bytes)?;
        }

        Ok(buffer)
    }

    /// Deserializes a buffer into a [`hyper::HeaderMap<HeaderValue>`] created from the `serialize()`
    /// function. See `serialize()` for details on the byte representation.
    ///
    /// This method may return an Error in a few cases:
    /// - It is not possible to read the number of headers in its u16 representation (big endian).
    /// - A header cannot be fully read. That is it's key and value pair.
    /// - The bytes cannot be read as valid UTF8.
    fn deserialize(buffer: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut cursor = Cursor::new(buffer);

        let headers_len = cursor.read_u16::<BigEndian>()?;

        let mut headers = HeaderMap::new();

        for _ in 0..headers_len {
            let name_len = cursor.read_u16::<BigEndian>()?;

            let mut name_bytes = vec![0u8; name_len as usize];
            cursor.read_exact(&mut name_bytes)?;

            let header_name = HeaderName::from_bytes(&name_bytes)?;

            let value_len = cursor.read_u16::<BigEndian>()?;

            let mut value_bytes = vec![0u8; value_len as usize];
            cursor.read_exact(&mut value_bytes)?;

            let value = String::from_utf8(value_bytes)?;

            headers.append(header_name, value.parse()?);
        }

        Ok(headers)
    }
}

impl ComponentBinaryProtocol for Bytes {
    /// Serializes [`Bytes`] in a byte representation. The structure of the returned buffer is 4 bytes
    /// (in big endian encoding) describing the byte length and N bytes for the content where
    /// N is the length encoded. You can use the accompanying `decode()` method to read this.
    ///
    /// Since 4 bytes are reserved for the length it cannot exceed a size of 2^32 -1.
    /// An Error result will be returned if so.
    ///
    /// This method will also return an Error result if the bytes cannot be written to the buffer
    /// as per the Vec `write_all()` method.
    fn serialize(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut buffer = Vec::new();

        let byte_len = self.len();

        if byte_len > u32::MAX as usize {
            return Err("Bytes exceeds maximum u32 representation of 2^32 − 1.".into());
        }

        // Write the length of the bytes using 4 bytes (big endian)
        buffer.write_u32::<BigEndian>(byte_len as u32)?;

        buffer.write_all(self)?;

        Ok(buffer)
    }

    /// Deserializes a buffer into a [`Bytes`] representation created from the `serialize()` function.
    /// See `serialize()` for details on the byte representation.
    ///
    /// This method may return an Error in a few cases:
    /// - It is not possible to read the byte length in its u32 representation (big endian).
    /// - The bytes (determined by the length) cannot be fully read or is interrupted.
    /// - The raw bytes cannot be read in `Bytes`.
    fn deserialize(buffer: &[u8]) -> Result<Self, Box<dyn std::error::Error>>
    where
        Self: std::marker::Sized,
    {
        let mut cursor = Cursor::new(buffer);

        let byte_len = cursor.read_u32::<BigEndian>()?;

        // Creates a vec of the expected size of our URI bytes
        let mut bytes_vec = vec![0u8; byte_len as usize];
        cursor.read_exact(&mut bytes_vec)?;

        let bytes = Bytes::from(bytes_vec);

        Ok(bytes)
    }
}

/// Serialize signature for [`hyper::Request<Incoming>`]
/// as a byte representation.
#[async_trait::async_trait]
pub trait RequestIncomingBinaryProtocol {
    async fn serialize(&mut self) -> Result<Vec<u8>, Box<dyn std::error::Error>>;
}

#[async_trait::async_trait]
impl RequestIncomingBinaryProtocol for Request<Incoming> {
    /// Serializes a [`hyper::Request<Incoming>`] into a byte representation.
    ///
    /// This will serialize the request's method, URI, headers, and body (if present). This
    /// can be deserialized into a [`hyper::Request<Bytes>`]. The reason for this is that a
    /// [`hyper::Request<Incoming>`] represents a stream of bytes and should not be instantiated
    /// directly. Since we have the bytes available it can be read directly.
    ///
    /// ## Example
    /// ```
    /// ...
    /// async fn handle_request(
    /// mut request: Request<hyper::body::Incoming>,
    /// ) -> Result<Response<Full<Bytes>>, Error> {
    ///
    /// 	let serialized = request
    /// 		.serialize()
    /// 		.await
    /// 		.expect("Failed to serialize request");}
    /// ```
    async fn serialize(&mut self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut buffer = Vec::new();

        // Serialize the method (as u8)
        let method_code = self.method().serialize()?;
        buffer.write_u8(method_code)?;

        // Serialize the URI (using BinaryProtocol for Uri)
        buffer.extend(self.uri().serialize()?);

        // Serialize the headers (using BinaryProtocol for HeaderMap)
        buffer.extend(self.headers().serialize()?);

        let body_bytes = self.collect().await?.to_bytes();
        buffer.extend(body_bytes.serialize()?);

        buffer.extend(body_bytes.serialize()?);

        Ok(buffer)
    }
}

/// Deserialize signature for [`hyper::Request<Incoming>`]
/// in it's byte representation.
#[async_trait::async_trait]
pub trait RequestBytesBinaryProtocol {
    async fn deserialize(buffer: &[u8]) -> Result<Request<Bytes>, Box<dyn std::error::Error>>;
}

#[async_trait::async_trait]
impl RequestBytesBinaryProtocol for Request<Bytes> {
    /// Deserializes a byte representation of a [`hyper::Request`] to it's original
    /// type.
    ///
    /// This is designed to be used against a serialized [`hyper::Request<Incoming>`]
    /// for which is exampled in the above types `serialize()` function.
    ///
    /// ## Example
    /// ```
    /// ...
    /// // Assuming you receive bytes (`Vec[u8]`) from somewhere where a
    /// // request was serialized.
    ///Ok(Message::Binary(bytes)) => {
    ///	println!("Received bytes");
    ///
    ///	// hyper::Request<Bytes>
    ///	let request = hyper::Request::<Bytes>::deserialize(&bytes).await.expect("Failed to deserialize request");
    ///},
    /// ```
    async fn deserialize(buffer: &[u8]) -> Result<Request<Bytes>, Box<dyn std::error::Error>> {
        println!("Buffer is: {:?}", buffer);
        let mut cursor = Cursor::new(buffer);

        // Deserialize the method (as a single u8)
        let method_code = cursor.read_u8()?;
        let method = Method::deserialize(method_code)?;

        // Deserialize the URI (2 bytes for len + content)
        let uri = Uri::deserialize(&buffer[cursor.position() as usize..])?;
        cursor.set_position(cursor.position() + 2 + uri.to_string().len() as u64);

        // Deserialize the headers (2 bytes for header count + X (headers_len) bytes for content)
        let headers = HeaderMap::deserialize(&buffer[cursor.position() as usize..])?;

        let headers_len = headers.iter().fold(0, |acc, (key, value)| {
            // 2 bytes for key len and then length of key
            let key_len = 2 + key.as_str().len();
            // Same as key but for value
            let value_len = 2 + value.as_bytes().len();
            acc + key_len + value_len
        });
        cursor.set_position(cursor.position() + 2 + headers_len as u64);

        // Deserialize the body as the remaining buffer.
        let body_bytes = Bytes::deserialize(&buffer[cursor.position() as usize..])?;

        let mut request = Request::builder().method(method).uri(uri.to_string());

        for (key, value) in headers.iter() {
            request = request.header(key, value);
        }

        let request = request.body(body_bytes)?;

        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_method_serializes_as_u8() {
        let get_method = Method::GET;
        let buffer_presentation = get_method.serialize().expect("Failed to serialize method");
        assert_eq!(buffer_presentation, 0);
    }

    #[test]
    fn u8_deserializes_as_post_method() {
        let u8_post_method = 0x01;
        let post_method =
            Method::deserialize(u8_post_method).expect("Failed to deserialize method");
        assert_eq!(post_method, Method::POST);
    }

    #[test]
    fn unmapped_u8_method_deserializes_as_err() {
        // 36 does not map to any known HTTP method
        let u8 = 0x24;
        let method_option = Method::deserialize(u8);
        assert!(method_option.is_err());
    }

    #[test]
    fn uri_serialization_roundtrip() {
        let uri = "/test/uri".parse::<Uri>().expect("Failed to parse URI");

        let encoded = uri.serialize().expect("Failed to encode URI");
        assert_eq!(encoded.len(), 11); // 2 bytes for length + 9 bytes for the URI

        let decoded_uri = Uri::deserialize(&encoded).expect("Failed to decode URI");
        assert_eq!(decoded_uri.to_string(), uri.to_string());
    }

    #[test]
    fn deserialization_of_invalid_length_uri_errors() {
        // Provide an invalid buffer where the length is too large for the actual URI data
        let invalid_buffer = vec![0x00, 0x10]; // Length of 16, but no URI data follows

        // Decoding should return an error
        let result = Uri::deserialize(&invalid_buffer);
        assert!(result.is_err());
    }

    #[test]
    fn deserialization_of_non_utf8_uri_errors() {
        let invalid_utf8 = vec![0x00, 0x03, 0xFF, 0xFE, 0xFD]; // 3-byte length, but invalid UTF-8

        // Decoding should return an error
        let result = Uri::deserialize(&invalid_utf8);
        assert!(result.is_err());

        if let Err(err) = result {
            assert!(
                err.downcast_ref::<std::str::Utf8Error>().is_some(),
                "Expected UTF-8 error"
            );
        }
    }

    #[test]
    fn uri_serialization_roundtrip_of_long_uri_is_successful() {
        // Test with a URI near the maximum possible length for a 2-byte length field (65535 bytes)
        let max_uri_length = 65_530;
        let uri = "x"
            .repeat(max_uri_length)
            .parse::<Uri>()
            .expect("Failed to parse URI");

        let encoded = uri.serialize().expect("Failed to encode max length URI");
        assert_eq!(encoded.len(), max_uri_length + 2); // 2 bytes for length + N bytes for URI

        let decoded_uri = Uri::deserialize(&encoded).expect("Failed to decode max length URI");
        assert_eq!(decoded_uri.to_string(), uri.to_string());
    }

    #[test]
    fn headers_serialization_roundtrip() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            HeaderName::from_static("user-agent"),
            HeaderValue::from_static("Rust"),
        );

        let encoded = headers.serialize().unwrap();
        let decoded = HeaderMap::<HeaderValue>::deserialize(&encoded).unwrap();

        assert_eq!(headers, decoded);
    }

    #[test]
    fn serialization_of_empty_header_map() {
        let headers = HeaderMap::new();

        let encoded = headers.serialize().unwrap();
        let decoded = HeaderMap::<HeaderValue>::deserialize(&encoded).unwrap();

        assert!(decoded.is_empty());
    }

    #[test]
    fn serialization_of_header_value_exceeding_max_length_errors() {
        let mut headers = HeaderMap::new();
        let large_value = "v".repeat(66_000);

        let result = headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_str(&large_value).unwrap(),
        );
        assert!(result.is_none()); // Inserting the large value should be valid at the API level

        let encode_result = headers.serialize();
        assert!(encode_result.is_err()); // Ensure encoding fails for values that exceed u16::MAX
    }

    #[test]
    fn deserialization_of_malformed_header_buffer_errors() {
        let malformed_input = vec![0, 1, 2, 3];

        let result = HeaderMap::<HeaderValue>::deserialize(&malformed_input);
        assert!(result.is_err());
    }

    #[test]
    fn deserialization_of_invalid_utf8_header_values_errors() {
        let mut headers = HeaderMap::new();
        let non_utf8_value = vec![0xFF, 0xFE, 0xFD]; // Invalid UTF-8 sequence

        let value = HeaderValue::from_bytes(&non_utf8_value).unwrap();
        headers.insert(HeaderName::from_static("custom-header"), value);

        let encoded = headers.serialize().unwrap();
        let decoded = HeaderMap::<HeaderValue>::deserialize(&encoded);

        assert!(decoded.is_err()); // Expect decoding to fail because of invalid UTF-8
    }

    #[test]
    fn bytes_serialization_roundtrip() {
        let original_data = Bytes::from("hello world");

        let encoded = original_data.serialize().expect("Encoding failed");
        assert_eq!(encoded.len(), 15); // 4 bytes for length + 11 bytes for "hello world"

        let decoded = Bytes::deserialize(&encoded).expect("Decoding failed");
        assert_eq!(decoded, original_data);
    }

    #[test]
    fn serialization_of_bytes_exceeding_u32_errors() {
        let oversized_bytes = Bytes::from(vec![0u8; (u32::MAX as usize) + 20]);

        // Encoding should fail because it exceeds the u32 limit
        let result = oversized_bytes.serialize();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Bytes exceeds maximum u32 representation of 2^32 − 1."
        );
    }

    #[test]
    fn deserialization_of_malformed_bytes_errors() {
        let original_data = Bytes::from("valid data");
        let mut encoded = original_data.serialize().expect("Encoding failed");

        // Truncatethe buffer to not match expected size
        encoded.truncate(5);

        let result = Bytes::deserialize(&encoded);
        assert!(result.is_err());
    }

    #[test]
    fn deserialization_of_invalid_length_bytes_errors() {
        let mut invalid_buffer = Vec::new();

        // Set an invalid length that is too large for the buffer
        invalid_buffer
            .write_u32::<BigEndian>(1000)
            .expect("Writing length failed");
        invalid_buffer.extend_from_slice(b"small"); // Only 5 bytes of actual data

        let result = Bytes::deserialize(&invalid_buffer);
        assert!(result.is_err());
    }
}
