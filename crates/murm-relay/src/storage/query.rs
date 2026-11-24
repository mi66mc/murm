use super::{RelayStore, events::expire};
use crate::{RelayConfig, StoreError, cursor::hash};
use murm_protocol::*;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value as Sql};
use serde_json::Value;

fn text(s: impl Into<String>) -> Sql {
    Sql::Text(s.into())
}
fn condition(c: &HeaderCondition, args: &mut Vec<Sql>) -> String {
    // Only the five implemented MIP-04 paths ever become SQLite JSON paths.
    let path = match c.path.as_str() {
        "/publication" => "$.header.publication",
        "/revision" => "$.header.revision",
        "/status" => "$.header.status",
        "/topics" => "$.header.topics",
        "/language" => "$.header.language",
        _ => return "0".into(),
    };
    let root = "CAST(e.json AS TEXT)";
    match c.op {
        HeaderOp::Contains => {
            let Some(value) = c.value.as_str() else {
                return "0".into();
            };
            args.push(text(path));
            args.push(text(path));
            args.push(text(value));
            format!(
                "(json_type({root},?)='array' AND EXISTS(SELECT 1 FROM json_each({root},?) item WHERE item.type='text' AND item.value=?))"
            )
        }
        HeaderOp::Eq => {
            args.push(text(path));
            match &c.value {
                Value::Null => format!("json_type({root},?)='null'"),
                Value::Bool(v) => format!(
                    "json_type({root},?)='{}'",
                    if *v { "true" } else { "false" }
                ),
                Value::String(v) => {
                    args.push(text(path));
                    args.push(text(v));
                    format!("(json_type({root},?)='text' AND json_extract({root},?)=?)")
                }
                Value::Number(n) => {
                    args.push(text(path));
                    args.push(Sql::Real(n.as_f64().unwrap_or(f64::NAN)));
                    format!(
                        "(json_type({root},?) IN ('integer','real') AND json_extract({root},?)=?)"
                    )
                }
                _ => {
                    args.pop();
                    "0".into()
                }
            }
        }
    }
}
fn filter_sql(filters: &[Filter], args: &mut Vec<Sql>) -> String {
    let mut branches = Vec::new();
    for f in filters {
        let mut ands = vec!["1".to_string()];
        if let Some(kinds) = &f.kinds {
            ands.push(format!("e.kind IN ({})", vec!["?"; kinds.len()].join(",")));
            args.extend(kinds.iter().map(|&n| Sql::Integer(n as i64)));
        }
        if let Some(signers) = &f.signers {
            ands.push(format!(
                "e.signer IN ({})",
                vec!["?"; signers.len()].join(",")
            ));
            args.extend(signers.iter().map(|v| text(v.to_string())));
        }
        if let Some(n) = f.since {
            ands.push("e.created_at>=?".into());
            args.push(Sql::Integer(n as i64));
        }
        if let Some(n) = f.until {
            ands.push("e.created_at<?".into());
            args.push(Sql::Integer(n as i64));
        }
        if let Some(cs) = &f.header {
            for c in cs {
                ands.push(condition(c, args));
            }
        }
        branches.push(format!("({})", ands.join(" AND ")));
    }
    format!("({})", branches.join(" OR "))
}
fn decode(bytes: &[u8]) -> Result<Event, StoreError> {
    Ok(parse_and_verify(
        bytes,
        ParseLimits {
            max_bytes: bytes.len(),
            max_depth: 128,
        },
    )?
    .into_event())
}
fn read(c: &Connection, id: &str) -> Result<Option<Event>, StoreError> {
    let bytes: Option<Vec<u8>> = c
        .query_row("SELECT json FROM events WHERE id=?1", [id], |r| r.get(0))
        .optional()?;
    bytes.map(|b| decode(&b)).transpose()
}
fn encoded_len(value: &impl serde::Serialize) -> Result<u64, StoreError> {
    Ok(serde_json::to_vec(value)
        .map_err(|_| StoreError::Unavailable)?
        .len() as u64)
}
impl RelayStore {
    pub fn query(
        &mut self,
        q: &EventQuery,
        c: &RelayConfig,
        now: u64,
    ) -> Result<QueryResponse, StoreError> {
        self.configure_capacity(c.max_storage_bytes)?;
        validate_query(q, &c.info)?;
        expire(&self.connection, now)?;
        let mut binding = q.clone();
        binding.cursor = None;
        let request_hash = hash(&binding)?;
        if q.view.is_some() {
            return self.current_query(q, c, now, &request_hash);
        }
        let mut cursor = match &q.cursor {
            Some(s) => {
                self.decode_cursor(s, "raw", &request_hash, now, c.info.limits.cursor_bytes)?
            }
            None => {
                self.new_cursor("raw", request_hash, now, c.info.limits.query_cursor_seconds)?
            }
        };
        cursor.expires = now.saturating_add(c.info.limits.query_cursor_seconds);
        let mut args = Vec::new();
        let where_sql = filter_sql(&q.filters, &mut args);
        let after = if q.cursor.is_some() {
            args.extend([
                Sql::Integer(cursor.time as i64),
                Sql::Integer(cursor.time as i64),
                text(&cursor.id),
            ]);
            " AND (e.created_at<? OR (e.created_at=? AND e.id<?))"
        } else {
            ""
        };
        args.push(Sql::Integer((q.limit + 1) as i64));
        let sql = format!(
            "SELECT e.json FROM events e WHERE {where_sql}{after} ORDER BY e.created_at DESC,e.id DESC LIMIT ?"
        );
        let mut stmt = self.connection.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(args))?;
        let mut response = QueryResponse {
            events: vec![],
            next_cursor: None,
        };
        let mut more = false;
        while let Some(row) = rows.next()? {
            if response.events.len() as u64 >= q.limit {
                more = true;
                break;
            }
            let event = decode(&row.get::<_, Vec<u8>>(0)?)?;
            let mut candidate = cursor.clone();
            candidate.time = event.created_at;
            candidate.id = event.id.to_string();
            response.events.push(event);
            response.next_cursor = Some(self.token(&candidate, c.info.limits.cursor_bytes)?);
            if encoded_len(&response)? > c.info.limits.response_bytes {
                response.events.pop();
                more = true;
                break;
            }
            cursor = candidate;
        }
        if more {
            if response.events.is_empty() {
                return Err(StoreError::Rejected(ProblemCode::LimitExceeded));
            }
            response.next_cursor = Some(self.token(&cursor, c.info.limits.cursor_bytes)?);
        } else {
            response.next_cursor = None;
        }
        Ok(response)
    }
    fn current_query(
        &mut self,
        q: &EventQuery,
        c: &RelayConfig,
        now: u64,
        request_hash: &str,
    ) -> Result<QueryResponse, StoreError> {
        let (mut cursor, ids) = if let Some(token) = &q.cursor {
            let cursor = self.decode_cursor(
                token,
                "current",
                request_hash,
                now,
                c.info.limits.cursor_bytes,
            )?;
            let id = cursor
                .snapshot
                .as_ref()
                .ok_or(StoreError::Rejected(ProblemCode::InvalidCursor))?;
            let row:Option<String>=self.connection.query_row("SELECT ids FROM query_snapshots WHERE id=?1 AND query_hash=?2 AND expires_at>?3",
                params![id,request_hash,now],|r|r.get(0)).optional()?;
            let ids: Vec<String> =
                serde_json::from_str(&row.ok_or(StoreError::Rejected(ProblemCode::CursorExpired))?)
                    .map_err(|_| StoreError::Unavailable)?;
            (cursor, ids)
        } else {
            let mut args = Vec::new();
            let where_sql = filter_sql(&q.filters, &mut args);
            args.push(Sql::Integer((c.info.limits.snapshot_events + 1) as i64));
            let sql = format!(
                "SELECT e.id FROM events e JOIN publication_heads h ON h.id=e.id WHERE {where_sql} ORDER BY e.created_at DESC,e.id DESC LIMIT ?"
            );
            let ids = self
                .connection
                .prepare(&sql)?
                .query_map(params_from_iter(args), |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            if ids.len() as u64 > c.info.limits.snapshot_events {
                return Err(StoreError::Rejected(ProblemCode::LimitExceeded));
            }
            if ids.len() as u64 <= q.limit {
                let mut complete = QueryResponse {
                    events: vec![],
                    next_cursor: None,
                };
                let mut fits = true;
                for id in &ids {
                    if let Some(event) = read(&self.connection, id)? {
                        complete.events.push(event);
                        if encoded_len(&complete)? > c.info.limits.response_bytes {
                            fits = false;
                            break;
                        }
                    }
                }
                // No continuation token was issued, so no durable snapshot is needed.
                if fits {
                    return Ok(complete);
                }
            }
            let (count, used): (u64, u64) = self.connection.query_row(
                "SELECT COUNT(*),COALESCE(SUM(event_count),0) FROM query_snapshots",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if count >= c.max_snapshots
                || used.saturating_add(ids.len() as u64) > c.max_snapshot_events
            {
                return Err(StoreError::Rejected(ProblemCode::LimitExceeded));
            }
            let mut cursor = self.new_cursor(
                "current",
                request_hash.into(),
                now,
                c.info.limits.query_cursor_seconds,
            )?;
            let snapshot = uuid::Uuid::new_v4().to_string();
            cursor.snapshot = Some(snapshot.clone());
            self.connection.execute("INSERT INTO query_snapshots(id,query_hash,ids,event_count,expires_at) VALUES(?1,?2,?3,?4,?5)",
                params![snapshot,request_hash,serde_json::to_string(&ids).map_err(|_|StoreError::Unavailable)?,ids.len() as u64,cursor.expires])?;
            (cursor, ids)
        };
        if cursor.position > ids.len() {
            return Err(StoreError::Rejected(ProblemCode::InvalidCursor));
        }
        let mut response = QueryResponse {
            events: vec![],
            next_cursor: None,
        };
        while cursor.position < ids.len() {
            let id = &ids[cursor.position];
            let Some(event) = read(&self.connection, id)? else {
                cursor.position += 1;
                continue;
            };
            if response.events.len() as u64 >= q.limit {
                break;
            }
            let mut candidate = cursor.clone();
            candidate.position += 1;
            response.events.push(event);
            response.next_cursor = Some(self.token(&candidate, c.info.limits.cursor_bytes)?);
            if encoded_len(&response)? > c.info.limits.response_bytes {
                response.events.pop();
                break;
            }
            cursor = candidate;
        }
        response.next_cursor = if cursor.position < ids.len() {
            if response.events.is_empty() {
                return Err(StoreError::Rejected(ProblemCode::LimitExceeded));
            }
            Some(self.token(&cursor, c.info.limits.cursor_bytes)?)
        } else {
            None
        };
        Ok(response)
    }
    pub fn sync(
        &mut self,
        q: &SyncRequest,
        c: &RelayConfig,
        now: u64,
    ) -> Result<SyncResponse, StoreError> {
        self.configure_capacity(c.max_storage_bytes)?;
        validate_sync(q, &c.info)?;
        expire(&self.connection, now)?;
        let request_hash = hash(&q.filters)?;
        let mut cursor = match &q.cursor {
            Some(token) => self.decode_cursor(
                token,
                "sync",
                &request_hash,
                now,
                c.info.limits.cursor_bytes,
            )?,
            None => {
                self.new_cursor("sync", request_hash, now, c.info.limits.sync_cursor_seconds)?
            }
        };
        cursor.expires = now.saturating_add(c.info.limits.sync_cursor_seconds);
        let high: u64 = self.connection.query_row(
            "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='events'),0)",
            [],
            |r| r.get(0),
        )?;
        if cursor.seq > high {
            return Err(StoreError::Rejected(ProblemCode::CursorExpired));
        }
        let mut args = Vec::new();
        let where_sql = filter_sql(&q.filters, &mut args);
        args.extend([
            Sql::Integer(cursor.seq as i64),
            Sql::Integer(high as i64),
            Sql::Integer((q.limit + 1) as i64),
        ]);
        let sql = format!(
            "SELECT e.admission_seq,e.json FROM events e WHERE {where_sql} AND e.admission_seq>? AND e.admission_seq<=? ORDER BY e.admission_seq LIMIT ?"
        );
        let mut stmt = self.connection.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(args))?;
        let mut response = SyncResponse {
            events: vec![],
            cursor: String::new(),
            has_more: false,
        };
        while let Some(row) = rows.next()? {
            if response.events.len() as u64 >= q.limit {
                response.has_more = true;
                break;
            }
            let mut candidate = cursor.clone();
            candidate.seq = row.get(0)?;
            response.events.push(decode(&row.get::<_, Vec<u8>>(1)?)?);
            response.cursor = self.token(&candidate, c.info.limits.cursor_bytes)?;
            if encoded_len(&response)? > c.info.limits.response_bytes {
                response.events.pop();
                response.has_more = true;
                break;
            }
            cursor = candidate;
        }
        if response.has_more && response.events.is_empty() {
            return Err(StoreError::Rejected(ProblemCode::LimitExceeded));
        }
        if !response.has_more {
            cursor.seq = high;
        }
        response.cursor = self.token(&cursor, c.info.limits.cursor_bytes)?;
        if encoded_len(&response)? > c.info.limits.response_bytes {
            return Err(StoreError::Rejected(ProblemCode::LimitExceeded));
        }
        Ok(response)
    }
}
