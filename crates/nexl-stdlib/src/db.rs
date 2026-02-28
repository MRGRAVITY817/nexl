//! `db` module — SQLite database access.
//!
//! Backed by `rusqlite` (bundled SQLite 3). Provides:
//!
//! - `(db/open path)` → `(Result Db DbError)` — open database (use `":memory:"` for in-memory)
//! - `(db/close db)` → `Unit` — close and release the database handle
//! - `(db/execute db sql params)` → `(Result Int DbError)` — DDL/DML statement
//! - `(db/query db sql params)` → `(Result (Vec Map) DbError)` — SELECT statement
//!
//! A `Db` handle is `Value::Adt { type_name: "Db", ctor: "Db", fields: [Int(id)] }`.
//! Connection objects are stored in a thread-local registry keyed by `id`.
//!
//! Parameterized queries use `?` placeholders; `params` is a Nexl `Vec` of values.
//! Only `Str`, `Int`, `Float`, `Bool`, and `Unit` (→ NULL) are supported as params.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use rusqlite::{Connection, ToSql, types::ToSqlOutput};

use nexl_runtime::Value;

use crate::StdlibEntry;

// ─── Thread-local connection registry ────────────────────────────────────────

thread_local! {
    static DB_REGISTRY: RefCell<HashMap<u64, Connection>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

fn insert_conn(conn: Connection) -> u64 {
    let id = NEXT_ID.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    });
    DB_REGISTRY.with(|reg| reg.borrow_mut().insert(id, conn));
    id
}

fn with_conn<F, T>(id: u64, f: F) -> Result<T, String>
where
    F: FnOnce(&Connection) -> Result<T, rusqlite::Error>,
{
    DB_REGISTRY.with(|reg| {
        let borrow = reg.borrow();
        let conn = borrow
            .get(&id)
            .ok_or_else(|| format!("db: invalid handle {id} (already closed?)"))?;
        f(conn).map_err(|e| format!("db error: {e}"))
    })
}

fn remove_conn(id: u64) -> bool {
    DB_REGISTRY.with(|reg| reg.borrow_mut().remove(&id).is_some())
}

// ─── Value helpers ────────────────────────────────────────────────────────────

/// Build a `Db` handle ADT from a connection id.
fn db_handle(id: u64) -> Value {
    Value::Adt {
        type_name: Rc::from("Db"),
        ctor: Rc::from("Db"),
        fields: Rc::new(vec![Value::Int(id as i64)]),
    }
}

/// Extract the connection id from a `Db` handle value.
fn extract_id(v: &Value) -> Result<u64, String> {
    match v {
        Value::Adt {
            type_name,
            ctor,
            fields,
        } if type_name.as_ref() == "Db"
            && ctor.as_ref() == "Db"
            && fields.len() == 1 =>
        {
            match &fields[0] {
                Value::Int(id) => Ok(*id as u64),
                _ => Err("db: malformed Db handle".to_string()),
            }
        }
        other => Err(format!(
            "db: expected Db handle, got {}",
            other.type_name()
        )),
    }
}

/// Wrap a value in `Ok(...)`.
fn ok_val(v: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Ok"),
        fields: Rc::new(vec![v]),
    }
}

/// Wrap an error string in `Err(...)`.
fn err_val(msg: &str) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Err"),
        fields: Rc::new(vec![Value::Str(Rc::from(msg))]),
    }
}

/// Build a Keyword value with no namespace.
fn kw(name: &str) -> Value {
    Value::Keyword {
        ns: None,
        name: Rc::from(name),
    }
}

// ─── Parameter conversion ─────────────────────────────────────────────────────

/// A wrapper that implements `rusqlite::ToSql` for Nexl `Value`.
struct SqlParam(Value);

impl ToSql for SqlParam {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        match &self.0 {
            Value::Int(n) => n.to_sql(),
            Value::Float(f) => f.to_sql(),
            Value::Str(s) => s.as_ref().to_sql(),
            Value::Bool(b) => Ok(ToSqlOutput::Owned(rusqlite::types::Value::Integer(
                if *b { 1 } else { 0 },
            ))),
            Value::Unit => Ok(ToSqlOutput::Owned(rusqlite::types::Value::Null)),
            other => Err(rusqlite::Error::ToSqlConversionFailure(
                format!("unsupported SQL param type: {}", other.type_name()).into(),
            )),
        }
    }
}

/// Extract a `Vec` of params from a `Value::Vec`, converting each to `SqlParam`.
fn extract_params(v: &Value) -> Result<Vec<SqlParam>, String> {
    match v {
        Value::Vec(items) => Ok(items.iter().map(|i| SqlParam(i.clone())).collect()),
        other => Err(format!(
            "db: params must be a Vec, got {}",
            other.type_name()
        )),
    }
}

// ─── Row conversion ───────────────────────────────────────────────────────────

/// Convert a rusqlite row to a `Value::Map` with keyword keys.
fn row_to_map(row: &rusqlite::Row<'_>, columns: &[String]) -> Result<Value, rusqlite::Error> {
    let mut entries = Vec::new();
    for (i, col) in columns.iter().enumerate() {
        let val: rusqlite::types::Value = row.get(i)?;
        let nexl_val = sqlite_value_to_nexl(val);
        entries.push((kw(col), nexl_val));
    }
    Ok(Value::Map(Rc::new(entries.into())))
}

/// Convert a `rusqlite::types::Value` to a Nexl `Value`.
fn sqlite_value_to_nexl(v: rusqlite::types::Value) -> Value {
    match v {
        rusqlite::types::Value::Null => Value::Unit,
        rusqlite::types::Value::Integer(n) => Value::Int(n),
        rusqlite::types::Value::Real(f) => Value::Float(f),
        rusqlite::types::Value::Text(s) => Value::Str(Rc::from(s.as_str())),
        rusqlite::types::Value::Blob(b) => {
            Value::Vec(Rc::new(b.into_iter().map(|byte| Value::Int(byte as i64)).collect()))
        }
    }
}

// ─── Stdlib entries ───────────────────────────────────────────────────────────

/// Return all `db` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("open", open_fn as fn(&[Value]) -> Result<Value, String>),
        ("close", close_fn),
        ("execute", execute_fn),
        ("query", query_fn),
    ]
}

/// `(db/open path)` — open a SQLite database. Use `":memory:"` for in-memory.
fn open_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(path)] => {
            match Connection::open(path.as_ref()) {
                Ok(conn) => {
                    let id = insert_conn(conn);
                    Ok(ok_val(db_handle(id)))
                }
                Err(e) => Ok(err_val(&format!("db/open failed: {e}"))),
            }
        }
        [other] => Err(format!(
            "`db/open` expected Str path, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`db/open` requires 1 argument (path), got {}",
            args.len()
        )),
    }
}

/// `(db/close db)` — close and release a database handle.
fn close_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [handle] => {
            let id = extract_id(handle)?;
            remove_conn(id);
            Ok(Value::Unit)
        }
        _ => Err(format!(
            "`db/close` requires 1 argument (db), got {}",
            args.len()
        )),
    }
}

/// `(db/execute db sql params)` — run a DDL/DML statement.
///
/// Returns `(Result Int DbError)` where Int is the number of rows affected.
fn execute_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [handle, Value::Str(sql), params_val] => {
            let id = extract_id(handle)?;
            let params = extract_params(params_val)?;
            let refs: Vec<&dyn ToSql> = params.iter().map(|p| p as &dyn ToSql).collect();
            match with_conn(id, |conn| conn.execute(sql, refs.as_slice())) {
                Ok(rows) => Ok(ok_val(Value::Int(rows as i64))),
                Err(e) => Ok(err_val(&e)),
            }
        }
        _ if args.len() != 3 => Err(format!(
            "`db/execute` requires 3 arguments (db sql params), got {}",
            args.len()
        )),
        _ => Err(format!(
            "`db/execute` expected (Db Str Vec), got ({}, {}, {})",
            args[0].type_name(),
            args[1].type_name(),
            args[2].type_name()
        )),
    }
}

/// `(db/query db sql params)` — run a SELECT statement.
///
/// Returns `(Result (Vec Map) DbError)` where each Map row has keyword keys.
fn query_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [handle, Value::Str(sql), params_val] => {
            let id = extract_id(handle)?;
            let params = extract_params(params_val)?;
            let refs: Vec<&dyn ToSql> = params.iter().map(|p| p as &dyn ToSql).collect();
            let result = with_conn(id, |conn| {
                let mut stmt = conn.prepare(sql)?;
                let columns: Vec<String> = stmt
                    .column_names()
                    .into_iter()
                    .map(String::from)
                    .collect();
                let rows: Result<Vec<Value>, _> = stmt
                    .query_map(refs.as_slice(), |row| row_to_map(row, &columns))?
                    .collect();
                rows
            });
            match result {
                Ok(rows) => Ok(ok_val(Value::Vec(Rc::new(rows)))),
                Err(e) => Ok(err_val(&e)),
            }
        }
        _ if args.len() != 3 => Err(format!(
            "`db/query` requires 3 arguments (db sql params), got {}",
            args.len()
        )),
        _ => Err(format!(
            "`db/query` expected (Db Str Vec), got ({}, {}, {})",
            args[0].type_name(),
            args[1].type_name(),
            args[2].type_name()
        )),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Open an in-memory database, extract the handle id, return both.
    fn open_memory() -> (Value, u64) {
        let result = open_fn(&[Value::Str(Rc::from(":memory:"))]).unwrap();
        let handle = match result {
            Value::Adt { ctor, ref fields, .. } if ctor.as_ref() == "Ok" => {
                fields[0].clone()
            }
            other => panic!("expected Ok, got {other}"),
        };
        let id = extract_id(&handle).unwrap();
        (handle, id)
    }

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_db_entries_registered() {
        let names: Vec<&str> = entries().iter().map(|(n, _)| *n).collect();
        for name in ["open", "close", "execute", "query"] {
            assert!(names.contains(&name), "missing entry: {name}");
        }
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_db_open_returns_ok_handle() {
        let result = open_fn(&[Value::Str(Rc::from(":memory:"))]).unwrap();
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "Ok"),
            other => panic!("expected Ok Adt, got {other}"),
        }
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_db_handle_is_adt_db() {
        let (handle, _) = open_memory();
        match handle {
            Value::Adt { type_name, ctor, .. } => {
                assert_eq!(type_name.as_ref(), "Db");
                assert_eq!(ctor.as_ref(), "Db");
            }
            other => panic!("expected Db Adt, got {other}"),
        }
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_db_execute_create_table() {
        let (handle, _) = open_memory();
        let empty_params = Value::Vec(Rc::new(vec![]));
        let result = execute_fn(&[
            handle,
            Value::Str(Rc::from("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)")),
            empty_params,
        ])
        .unwrap();
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "Ok"),
            other => panic!("expected Ok, got {other}"),
        }
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_db_execute_returns_rows_affected() {
        let (handle, _) = open_memory();
        let empty = Value::Vec(Rc::new(vec![]));
        execute_fn(&[
            handle.clone(),
            Value::Str(Rc::from("CREATE TABLE t2 (id INTEGER, val TEXT)")),
            empty.clone(),
        ])
        .unwrap();
        let result = execute_fn(&[
            handle,
            Value::Str(Rc::from("INSERT INTO t2 VALUES (1, 'hello')")),
            empty,
        ])
        .unwrap();
        match result {
            Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Ok" => {
                assert_eq!(fields[0], Value::Int(1), "should have 1 row affected");
            }
            other => panic!("expected Ok(Int(1)), got {other}"),
        }
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_db_query_empty_table() {
        let (handle, _) = open_memory();
        let empty = Value::Vec(Rc::new(vec![]));
        execute_fn(&[
            handle.clone(),
            Value::Str(Rc::from("CREATE TABLE t3 (x INTEGER)")),
            empty.clone(),
        ])
        .unwrap();
        let result = query_fn(&[
            handle,
            Value::Str(Rc::from("SELECT * FROM t3")),
            empty,
        ])
        .unwrap();
        match result {
            Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Ok" => {
                match &fields[0] {
                    Value::Vec(rows) => assert!(rows.is_empty(), "empty table should return 0 rows"),
                    other => panic!("expected Vec rows, got {other}"),
                }
            }
            other => panic!("expected Ok(Vec), got {other}"),
        }
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_db_query_returns_row_map() {
        let (handle, _) = open_memory();
        let empty = Value::Vec(Rc::new(vec![]));
        execute_fn(&[
            handle.clone(),
            Value::Str(Rc::from("CREATE TABLE t4 (id INTEGER, name TEXT)")),
            empty.clone(),
        ])
        .unwrap();
        execute_fn(&[
            handle.clone(),
            Value::Str(Rc::from("INSERT INTO t4 VALUES (42, 'nexl')")),
            empty.clone(),
        ])
        .unwrap();
        let result = query_fn(&[
            handle,
            Value::Str(Rc::from("SELECT id, name FROM t4")),
            empty,
        ])
        .unwrap();
        let rows = match result {
            Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Ok" => match &fields[0] {
                Value::Vec(rows) => rows.clone(),
                other => panic!("expected Vec, got {other}"),
            },
            other => panic!("expected Ok, got {other}"),
        };
        assert_eq!(rows.len(), 1, "should have 1 row");
        let row = &rows[0];
        match row {
            Value::Map(m) => {
                // Row should have :id = 42 and :name = "nexl"
                let id_key = Value::Keyword { ns: None, name: Rc::from("id") };
                let name_key = Value::Keyword { ns: None, name: Rc::from("name") };
                let id_val = m.iter().find(|(k, _)| *k == &id_key).map(|(_, v)| v.clone());
                let name_val = m.iter().find(|(k, _)| *k == &name_key).map(|(_, v)| v.clone());
                assert_eq!(id_val, Some(Value::Int(42)));
                assert_eq!(name_val, Some(Value::Str(Rc::from("nexl"))));
            }
            other => panic!("expected row Map, got {other}"),
        }
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_db_execute_parameterized() {
        let (handle, _) = open_memory();
        let empty = Value::Vec(Rc::new(vec![]));
        execute_fn(&[
            handle.clone(),
            Value::Str(Rc::from("CREATE TABLE t5 (id INTEGER, val TEXT)")),
            empty,
        ])
        .unwrap();
        let params = Value::Vec(Rc::new(vec![Value::Int(7), Value::Str(Rc::from("hello"))]));
        let result = execute_fn(&[
            handle,
            Value::Str(Rc::from("INSERT INTO t5 VALUES (?, ?)")),
            params,
        ])
        .unwrap();
        match result {
            Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Ok" => {
                assert_eq!(fields[0], Value::Int(1));
            }
            other => panic!("expected Ok(Int(1)), got {other}"),
        }
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_db_query_parameterized() {
        let (handle, _) = open_memory();
        let empty = Value::Vec(Rc::new(vec![]));
        execute_fn(&[
            handle.clone(),
            Value::Str(Rc::from("CREATE TABLE t6 (id INTEGER, name TEXT)")),
            empty.clone(),
        ])
        .unwrap();
        execute_fn(&[
            handle.clone(),
            Value::Str(Rc::from("INSERT INTO t6 VALUES (1, 'alice'), (2, 'bob')")),
            empty,
        ])
        .unwrap();
        let params = Value::Vec(Rc::new(vec![Value::Int(1)]));
        let result = query_fn(&[
            handle,
            Value::Str(Rc::from("SELECT name FROM t6 WHERE id = ?")),
            params,
        ])
        .unwrap();
        let rows = match result {
            Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Ok" => match &fields[0] {
                Value::Vec(rows) => rows.clone(),
                other => panic!("expected Vec, got {other}"),
            },
            other => panic!("expected Ok, got {other}"),
        };
        assert_eq!(rows.len(), 1);
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_db_close_returns_unit() {
        let (handle, _) = open_memory();
        let result = close_fn(&[handle]).unwrap();
        assert_eq!(result, Value::Unit);
    }

    // ── Test 11 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_db_open_wrong_arg() {
        let err = open_fn(&[Value::Int(42)]).unwrap_err();
        assert!(err.contains("Str"), "error should mention Str: {err}");
    }

    // ── Test 12 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_db_invalid_handle_query() {
        let bad_handle = db_handle(999999);
        let empty = Value::Vec(Rc::new(vec![]));
        let result = query_fn(&[
            bad_handle,
            Value::Str(Rc::from("SELECT 1")),
            empty,
        ])
        .unwrap();
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "Err"),
            other => panic!("expected Err, got {other}"),
        }
    }

    // ── Test 13 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_db_close_removes_handle() {
        let (handle, _) = open_memory();
        let empty = Value::Vec(Rc::new(vec![]));
        close_fn(&[handle.clone()]).unwrap();
        // After close, query should return Err
        let result = query_fn(&[
            handle,
            Value::Str(Rc::from("SELECT 1")),
            empty,
        ])
        .unwrap();
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "Err"),
            other => panic!("expected Err after close, got {other}"),
        }
    }
}
