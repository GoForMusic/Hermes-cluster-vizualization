use super::*;
use tokio::sync::mpsc;

#[test]
fn a_line_that_is_too_long_is_cut_where_a_character_ends_and_says_so() {
    assert_eq!(clip("short".into()), "short");
    let cut = clip("é".repeat(MAX_LINE));
    assert!(cut.ends_with(" … [line cut]"));
    assert!(cut.len() < MAX_LINE + 20);
    assert!(cut.is_char_boundary(cut.len()), "still valid text");
}

#[tokio::test]
async fn lines_go_out_in_batches_and_what_is_left_goes_when_the_reader_stops() {
    let (tx, mut rx) = mpsc::channel(8);
    let mut b = Batcher::new(tx);
    for i in 0..(CHUNK_LINES + 3) {
        assert!(b.push(format!("line {i}")).await);
    }
    let first = rx.recv().await.unwrap();
    assert_eq!((first.len(), first[0].as_str()), (CHUNK_LINES, "line 0"));
    assert!(rx.try_recv().is_err(), "the last three wait for the flush");
    assert!(b.flush().await);
    assert_eq!(
        rx.recv().await.unwrap(),
        vec!["line 500", "line 501", "line 502"]
    );
    assert!(b.flush().await, "nothing to flush is not a failure");
}

#[tokio::test]
async fn a_viewer_that_left_is_noticed_by_the_next_batch() {
    let (tx, rx) = mpsc::channel(1);
    let mut b = Batcher::new(tx);
    drop(rx);
    b.push("x".into()).await;
    assert!(!b.flush().await);
}
