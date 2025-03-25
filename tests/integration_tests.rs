use devdocs_mcp::{tools::DocRouter, transport::jsonrpc_frame_codec::JsonRpcFrameCodec};
use mcp_server::Router;
use serde_json::{json, Value};
use tokio_util::codec::Decoder;

#[tokio::test]
async fn test_doc_router_initialization() {
    let router = DocRouter::new();
    
    assert_eq!(router.name(), "codenav-docs");
    assert!(router.capabilities().tools.is_some());
    
    let tools = router.list_tools();
    assert!(tools.len() > 0);
    
    let lookup_crate_tool = tools.iter().find(|t| t.name == "lookup_crate").unwrap();
    let schema: Value = serde_json::from_value(lookup_crate_tool.input_schema.clone()).unwrap();
    assert_eq!(schema["type"], "object");
    assert!(schema["required"].as_array().unwrap().contains(&json!("crate_name")));
}

#[test]
fn test_jsonrpc_codec_functionality() {
    let mut codec = JsonRpcFrameCodec;
    let json_rpc = r#"{"jsonrpc":"2.0","method":"lookup_crate","params":{"crate_name":"tokio"},"id":1}"#;
    
    let mut buffer = tokio_util::bytes::BytesMut::from(json_rpc);
    buffer.extend_from_slice(b"\n");
    
    let decoded = codec.decode(&mut buffer).unwrap().unwrap();
    assert_eq!(decoded, json_rpc);
}

#[tokio::test]
async fn test_invalid_parameters_handling() {
    let router = DocRouter::new();
    
    let result = router.call_tool("lookup_crate", json!({})).await;
    assert!(matches!(result, Err(mcp_core::ToolError::InvalidParameters(_))));
    
    let result = router.call_tool("invalid_tool", json!({})).await;
    assert!(matches!(result, Err(mcp_core::ToolError::NotFound(_))));
}

// This test requires network access
#[tokio::test]
#[ignore = "Requires network access"]
async fn test_end_to_end_crate_lookup() {
    let router = DocRouter::new();
    
    let result = router.call_tool(
        "lookup_crate", 
        json!({
            "crate_name": "serde"
        })
    ).await;
    
    assert!(result.is_ok());
    let content = result.unwrap();
    assert_eq!(content.len(), 1);
    
    match &content[0] {
        mcp_core::Content::Text(text) => {
            assert!(text.text.contains("<!DOCTYPE html>"));
            assert!(text.text.contains("serde"));
        },
        _ => panic!("Expected text content"),
    }
}

// Test resource and prompt API error cases (since they're not implemented)
#[tokio::test]
async fn test_unimplemented_apis() {
    let router = DocRouter::new();
    
    assert!(router.list_resources().is_empty());
    
    let result = router.read_resource("test").await;
    assert!(result.is_err());
    
    assert!(router.list_prompts().is_empty());
    
    let result = router.get_prompt("test").await;
    assert!(result.is_err());
}