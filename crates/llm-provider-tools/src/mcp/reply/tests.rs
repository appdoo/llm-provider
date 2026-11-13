use serde_json::json;

use super::*;

fn reply(blocks: Vec<Block>) -> Reply {
    Reply {
        blocks,
        structured: None,
        is_error: false,
    }
}

#[test]
fn text_blocks_are_joined_with_blank_lines() {
    let output = reply(vec![Block::Text("one".into()), Block::Text("two".into())]).into_output();

    assert_eq!(
        output,
        ToolOutput::ok(
            "one

two"
        )
    );
}

#[test]
fn image_and_audio_blocks_show_their_media_type() {
    let output = reply(vec![
        Block::Image {
            mime_type: "image/png".into(),
        },
        Block::Audio {
            mime_type: "audio/wav".into(),
        },
    ])
    .into_output();

    assert_eq!(output.content, "[image image/png]\n\n[audio audio/wav]");
}

#[test]
fn a_resource_shows_its_uri_and_its_text() {
    let output = reply(vec![
        Block::Resource {
            uri: "file:///notes.txt".into(),
            text: Some("the notes".into()),
        },
        Block::Resource {
            uri: "file:///logo.png".into(),
            text: None,
        },
    ])
    .into_output();

    assert_eq!(
        output.content,
        "[resource file:///notes.txt]\nthe notes\n\n[resource file:///logo.png]"
    );
}

#[test]
fn structured_content_stands_in_when_there_is_no_text() {
    let output = Reply {
        blocks: Vec::new(),
        structured: Some(json!({"sum": 5})),
        is_error: false,
    }
    .into_output();

    assert_eq!(output.content, r#"{"sum":5}"#);
}

#[test]
fn text_wins_over_structured_content() {
    let output = Reply {
        blocks: vec![Block::Text("5".into())],
        structured: Some(json!({"sum": 5})),
        is_error: false,
    }
    .into_output();

    assert_eq!(output.content, "5");
}

#[test]
fn the_server_error_flag_is_kept() {
    let output = Reply {
        blocks: vec![Block::Text("broken".into())],
        structured: None,
        is_error: true,
    }
    .into_output();

    assert!(output.is_error);
    assert_eq!(output.content, "broken");
}
