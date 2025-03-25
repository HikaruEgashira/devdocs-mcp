
use crate::transport::jsonrpc_frame_codec::JsonRpcFrameCodec;
use tokio_util::bytes::BytesMut;
use tokio_util::codec::Decoder;

#[test]
fn test_decode_single_line() {
    let mut codec = JsonRpcFrameCodec::default();
    let mut buffer = BytesMut::from(r#"{"jsonrpc":"2.0","method":"test"}"#);
    buffer.extend_from_slice(b"\n");
    
    let result = codec.decode(&mut buffer).unwrap();
    
    // Should decode successfully
    assert!(result.is_some());
    let bytes = result.unwrap();
    assert_eq!(bytes, r#"{"jsonrpc":"2.0","method":"test"}"#);
    
    // Buffer should be empty after decoding
    assert_eq!(buffer.len(), 0);
}

#[test]
fn test_decode_incomplete_frame() {
    let mut codec = JsonRpcFrameCodec::default();
    let mut buffer = BytesMut::from(r#"{"jsonrpc":"2.0","method":"test""#);
    
    // Should return None when no newline is found
    let result = codec.decode(&mut buffer).unwrap();
    assert!(result.is_none());
    
    // Buffer should still contain the incomplete frame
    assert_eq!(buffer.len(), 32);
}

#[test]
fn test_decode_multiple_frames() {
    let mut codec = JsonRpcFrameCodec::default();
    let json1 = r#"{"jsonrpc":"2.0","method":"test1"}"#;
    let json2 = r#"{"jsonrpc":"2.0","method":"test2"}"#;
    
    let mut buffer = BytesMut::new();
    buffer.extend_from_slice(json1.as_bytes());
    buffer.extend_from_slice(b"\n");
    buffer.extend_from_slice(json2.as_bytes());
    buffer.extend_from_slice(b"\n");
    
    // First decode should return the first frame
    let result1 = codec.decode(&mut buffer).unwrap();
    assert!(result1.is_some());
    assert_eq!(result1.unwrap(), json1);
    
    // Second decode should return the second frame
    let result2 = codec.decode(&mut buffer).unwrap();
    assert!(result2.is_some());
    assert_eq!(result2.unwrap(), json2);
    
    // Buffer should be empty after decoding both frames
    assert_eq!(buffer.len(), 0);
}

#[test]
fn test_decode_empty_line() {
    let mut codec = JsonRpcFrameCodec::default();
    let mut buffer = BytesMut::from("\n");
    
    // Should return an empty frame
    let result = codec.decode(&mut buffer).unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().len(), 0);
    
    // Buffer should be empty
    assert_eq!(buffer.len(), 0);
}