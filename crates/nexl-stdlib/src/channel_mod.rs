//! `channel` module — CSP-style channels (Stage 0: single-threaded, thread-local).
//!
//! Channels are represented as `(Channel id)` ADTs where `id` is an `Int`.
//! The underlying queue lives in a thread-local registry keyed by that id.
//!
//! Functions:
//! - `(channel/new)` — unbuffered channel (capacity 1)
//! - `(channel/buffered n)` — buffered channel (capacity n)
//! - `(channel/send! ch val)` — send a value; Err if full or closed
//! - `(channel/recv!)` — receive (Option Val); None if empty
//! - `(channel/try-send! ch val)` — non-blocking send
//! - `(channel/try-recv! ch)` — non-blocking receive
//! - `(channel/close! ch)` — mark closed; further sends fail
//! - `(channel/closed? ch)` — Bool

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use nexl_runtime::Value;

use crate::StdlibEntry;

// ─── Registry ─────────────────────────────────────────────────────────────────

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct ChannelState {
    queue: VecDeque<Value>,
    capacity: usize,
    closed: bool,
}

thread_local! {
    static CHANNELS: RefCell<HashMap<u64, ChannelState>> = RefCell::new(HashMap::new());
}

fn new_channel(capacity: usize) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    CHANNELS.with(|c| {
        c.borrow_mut().insert(
            id,
            ChannelState {
                queue: VecDeque::new(),
                capacity,
                closed: false,
            },
        );
    });
    id
}

fn extract_id(ch: &Value) -> Result<u64, String> {
    match ch {
        Value::Adt { type_name, ctor, fields }
            if type_name.as_ref() == "Channel" && ctor.as_ref() == "Channel" =>
        {
            match fields.first() {
                Some(Value::Int(id)) => Ok(*id as u64),
                _ => Err("`Channel` ADT has unexpected field type".into()),
            }
        }
        other => Err(format!("expected Channel, got {}", other.type_name())),
    }
}

fn channel_adt(id: u64) -> Value {
    Value::Adt {
        type_name: Rc::from("Channel"),
        ctor: Rc::from("Channel"),
        fields: Rc::new(vec![Value::Int(id as i64)]),
    }
}

fn ok_val(v: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Ok"),
        fields: Rc::new(vec![v]),
    }
}

fn err_val(msg: &str) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Err"),
        fields: Rc::new(vec![Value::Str(Rc::from(msg))]),
    }
}

fn some_val(v: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Option"),
        ctor: Rc::from("Some"),
        fields: Rc::new(vec![v]),
    }
}

fn none_val() -> Value {
    Value::Adt {
        type_name: Rc::from("Option"),
        ctor: Rc::from("None"),
        fields: Rc::new(vec![]),
    }
}

// ─── Entries ──────────────────────────────────────────────────────────────────

/// Return all `channel` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("new", new_fn as fn(&[Value]) -> Result<Value, String>),
        ("buffered", buffered_fn),
        ("send!", send_fn),
        ("recv!", recv_fn),
        ("try-send!", try_send_fn),
        ("try-recv!", try_recv_fn),
        ("close!", close_fn),
        ("closed?", closed_pred),
    ]
}

// ─── Implementations ──────────────────────────────────────────────────────────

/// `(channel/new)` — create an unbuffered channel (capacity 1).
fn new_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`channel/new` takes 0 arguments, got {}", args.len()));
    }
    Ok(channel_adt(new_channel(1)))
}

/// `(channel/buffered n)` — create a buffered channel with capacity `n`.
fn buffered_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(n)] if *n > 0 => Ok(channel_adt(new_channel(*n as usize))),
        [Value::Int(n)] => Err(format!(
            "`channel/buffered` capacity must be positive, got {n}"
        )),
        [other] => Err(format!(
            "`channel/buffered` expected Int capacity, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`channel/buffered` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(channel/send! ch val)` — enqueue `val`; returns `(Ok Unit)` or `(Err Str)`.
fn send_fn(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err(format!(
            "`channel/send!` requires 2 arguments, got {}",
            args.len()
        ));
    }
    let id = extract_id(&args[0])?;
    let val = args[1].clone();
    let result = CHANNELS.with(|c| {
        let mut map = c.borrow_mut();
        let state = map
            .get_mut(&id)
            .ok_or_else(|| format!("channel {id} not found"))?;
        if state.closed {
            return Err("send on closed channel".to_string());
        }
        if state.queue.len() >= state.capacity {
            return Err("channel is full".to_string());
        }
        state.queue.push_back(val);
        Ok(())
    });
    Ok(match result {
        Ok(()) => ok_val(Value::Unit),
        Err(e) => err_val(&e),
    })
}

/// `(channel/recv! ch)` — dequeue; returns `(Some val)` or `None`.
fn recv_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [ch] => {
            let id = extract_id(ch)?;
            let result = CHANNELS.with(|c| -> Result<Option<Value>, String> {
                let mut map = c.borrow_mut();
                let state = map.get_mut(&id).ok_or_else(|| format!("channel {id} not found"))?;
                Ok(state.queue.pop_front())
            })?;
            Ok(match result {
                Some(v) => some_val(v),
                None => none_val(),
            })
        }
        _ => Err(format!(
            "`channel/recv!` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(channel/try-send! ch val)` — non-blocking send; same as `send!` in Stage 0.
fn try_send_fn(args: &[Value]) -> Result<Value, String> {
    send_fn(args)
}

/// `(channel/try-recv! ch)` — non-blocking receive; same as `recv!` in Stage 0.
fn try_recv_fn(args: &[Value]) -> Result<Value, String> {
    recv_fn(args)
}

/// `(channel/close! ch)` — mark channel closed.
fn close_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [ch] => {
            let id = extract_id(ch)?;
            CHANNELS.with(|c| {
                let mut map = c.borrow_mut();
                if let Some(state) = map.get_mut(&id) {
                    state.closed = true;
                }
            });
            Ok(Value::Unit)
        }
        _ => Err(format!(
            "`channel/close!` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(channel/closed? ch)` — `true` if channel is closed.
fn closed_pred(args: &[Value]) -> Result<Value, String> {
    match args {
        [ch] => {
            let id = extract_id(ch)?;
            let closed = CHANNELS.with(|c| {
                let map = c.borrow();
                map.get(&id).map(|s| s.closed).unwrap_or(true)
            });
            Ok(Value::Bool(closed))
        }
        _ => Err(format!(
            "`channel/closed?` requires 1 argument, got {}",
            args.len()
        )),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_channel(capacity: usize) -> Value {
        channel_adt(new_channel(capacity))
    }

    // 1. new_fn returns a Channel ADT
    #[test]
    fn test_new_returns_channel() {
        let ch = new_fn(&[]).unwrap();
        assert!(
            matches!(&ch, Value::Adt { type_name, ctor, .. }
                if type_name.as_ref() == "Channel" && ctor.as_ref() == "Channel"),
            "expected Channel ADT, got {ch:?}"
        );
    }

    // 2. buffered_fn with capacity > 0 succeeds
    #[test]
    fn test_buffered_valid_capacity() {
        let ch = buffered_fn(&[Value::Int(5)]).unwrap();
        assert!(matches!(&ch, Value::Adt { type_name, .. } if type_name.as_ref() == "Channel"));
    }

    // 3. buffered_fn with capacity 0 returns error
    #[test]
    fn test_buffered_zero_capacity_error() {
        assert!(buffered_fn(&[Value::Int(0)]).is_err());
    }

    // 4. send then recv round-trips a value
    #[test]
    fn test_send_recv_roundtrip() {
        let ch = make_channel(4);
        let sent = send_fn(&[ch.clone(), Value::Int(42)]).unwrap();
        assert!(
            matches!(&sent, Value::Adt { ctor, .. } if ctor.as_ref() == "Ok"),
            "send should return Ok"
        );
        let received = recv_fn(&[ch]).unwrap();
        match received {
            Value::Adt { ctor, fields, .. } => {
                assert_eq!(ctor.as_ref(), "Some");
                assert_eq!(fields[0], Value::Int(42));
            }
            other => panic!("expected Some(42), got {other:?}"),
        }
    }

    // 5. recv on empty returns None
    #[test]
    fn test_recv_empty_returns_none() {
        let ch = make_channel(2);
        let result = recv_fn(&[ch]).unwrap();
        assert!(
            matches!(&result, Value::Adt { ctor, .. } if ctor.as_ref() == "None"),
            "expected None on empty channel"
        );
    }

    // 6. send on full channel returns Err
    #[test]
    fn test_send_full_returns_err() {
        let ch = make_channel(1);
        send_fn(&[ch.clone(), Value::Int(1)]).unwrap();
        let result = send_fn(&[ch, Value::Int(2)]).unwrap();
        assert!(
            matches!(&result, Value::Adt { ctor, .. } if ctor.as_ref() == "Err"),
            "expected Err when channel full"
        );
    }

    // 7. close! then send returns Err
    #[test]
    fn test_send_after_close_returns_err() {
        let ch = make_channel(4);
        close_fn(&[ch.clone()]).unwrap();
        let result = send_fn(&[ch, Value::Int(1)]).unwrap();
        assert!(
            matches!(&result, Value::Adt { ctor, .. } if ctor.as_ref() == "Err"),
            "expected Err on closed channel send"
        );
    }

    // 8. closed? returns true after close!
    #[test]
    fn test_closed_pred() {
        let ch = make_channel(2);
        assert_eq!(closed_pred(&[ch.clone()]).unwrap(), Value::Bool(false));
        close_fn(&[ch.clone()]).unwrap();
        assert_eq!(closed_pred(&[ch]).unwrap(), Value::Bool(true));
    }

    // 9. entries registered with correct names
    #[test]
    fn test_entries_registered() {
        let names: Vec<&str> = entries().iter().map(|(n, _)| *n).collect();
        for name in ["new", "buffered", "send!", "recv!", "try-send!", "try-recv!", "close!", "closed?"] {
            assert!(names.contains(&name), "missing: {name}");
        }
    }

    // 10. FIFO ordering preserved
    #[test]
    fn test_fifo_ordering() {
        let ch = make_channel(8);
        for i in 0i64..4 {
            send_fn(&[ch.clone(), Value::Int(i)]).unwrap();
        }
        for i in 0i64..4 {
            let v = recv_fn(&[ch.clone()]).unwrap();
            match v {
                Value::Adt { ctor, fields, .. } => {
                    assert_eq!(ctor.as_ref(), "Some");
                    assert_eq!(fields[0], Value::Int(i));
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }
}
