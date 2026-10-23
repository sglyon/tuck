//! Local projections: bounded pages, global scalar counts and capped facets.
use super::{decode, storage, SqlRepository};
use crate::{domain::Meeting, library::LibraryQuery, ports::Result};
use rusqlite::{params_from_iter, types::Value as SqlValue};
use serde_json::{json, Value};

const HEADER: &str = "json_object(
 'id',m.id,'title',json_extract(m.document,'$.title'),'kind',coalesce(json_extract(m.document,'$.kind'),'meeting'),'createdAt',m.created_at,
 'duration',json_extract(m.document,'$.duration'),'status',json_extract(m.document,'$.status'),
 'template',json_extract(m.document,'$.template'),'starred',json(CASE WHEN json_extract(m.document,'$.starred') THEN 'true' ELSE 'false' END),
 'isExample',json(CASE WHEN json_extract(m.document,'$.isExample') THEN 'true' ELSE 'false' END),
 'notionUrl',coalesce(json_extract(m.document,'$.notionUrl'),''),'notionStatus',coalesce(json_extract(m.document,'$.notionStatus'),'idle'),
 'folder',coalesce(json_extract(m.document,'$.folder'),''),'tags',json(coalesce(json_extract(m.document,'$.tags'),'[]')),
 'people',json(coalesce(json_extract(m.document,'$.people'),'[]')),'calendarEvent',json(json_extract(m.document,'$.calendarEvent')),
 'recoveredFrom',json_extract(m.document,'$.recoveredFrom'),
 'hasNotes',json(CASE WHEN length(coalesce(json_extract(m.document,'$.notes'),''))>0 THEN 'true' ELSE 'false' END),
 'hasSummary',json(CASE WHEN length(coalesce(json_extract(m.document,'$.summary'),''))>0 THEN 'true' ELSE 'false' END),
 'taskCount',coalesce(json_array_length(m.document,'$.tasks'),0),
 'openTaskCount',(SELECT count(*) FROM json_each(m.document,'$.tasks') t WHERE coalesce(json_extract(t.value,'$.done'),0)=0),
 'segmentCount',(SELECT count(*) FROM segments s WHERE s.meeting_id=m.id),
 'speakerCount',(SELECT count(*) FROM speakers s WHERE s.meeting_id=m.id),
 'messageCount',(SELECT count(*) FROM messages s WHERE s.meeting_id=m.id),
 'bookmarkCount',(SELECT count(*) FROM bookmarks s WHERE s.meeting_id=m.id))";

fn filters(query: &LibraryQuery, tasks: bool) -> (String, Vec<SqlValue>) {
    let mut clauses = vec!["1".to_owned()];
    let mut values = Vec::new();
    let mut push = |clause: String, value: String| {
        clauses.push(clause);
        values.push(SqlValue::Text(value));
    };
    if !query.scope.folder.is_empty() {
        push(
            "coalesce(json_extract(m.document,'$.folder'),'')=?".into(),
            query.scope.folder.clone(),
        );
    }
    for (field, list) in [("tags", &query.scope.tags), ("people", &query.scope.people)] {
        for value in list {
            push(
                format!(
                    "EXISTS(SELECT 1 FROM json_each(m.document,'$.{field}') f WHERE f.value=?)"
                ),
                value.clone(),
            );
        }
    }
    if !query.scope.meeting_ids.is_empty() {
        clauses.push(format!(
            "m.id IN ({})",
            vec!["?"; query.scope.meeting_ids.len()].join(",")
        ));
        values.extend(query.scope.meeting_ids.iter().cloned().map(SqlValue::Text));
    }
    if !tasks {
        if let Some(starred) = query.starred {
            clauses.push("coalesce(json_extract(m.document,'$.starred'),0)=?".into());
            values.push(SqlValue::Integer(i64::from(starred)));
        }
        if !query.query.is_empty() {
            clauses.push("(instr(lower(coalesce(json_extract(m.document,'$.title'),'')),?)>0 OR instr(lower(coalesce(json_extract(m.document,'$.notes'),'')),?)>0 OR instr(lower(coalesce(json_extract(m.document,'$.summary'),'')),?)>0 OR EXISTS(SELECT 1 FROM segments s LEFT JOIN transcript_corrections c ON c.meeting_id=s.meeting_id AND c.segment_id=s.id WHERE s.meeting_id=m.id AND instr(lower(coalesce(json_extract(c.document,'$.text'),json_extract(s.document,'$.text'))),?)>0))".into());
            values.extend(std::iter::repeat_n(SqlValue::Text(query.query.clone()), 4));
        }
    }
    (clauses.join(" AND "), values)
}
impl SqlRepository<'_> {
    fn generation(&self) -> Result<i64> {
        self.db
            .query_row(
                "SELECT generation FROM library_generation WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(storage)
    }
    pub(super) fn recoverable_meetings(&self) -> Result<Vec<Meeting>> {
        let mut statement = self.db.prepare("SELECT document,version FROM meetings WHERE json_extract(document,'$.status') IN ('recording','paused','processing') OR json_extract(document,'$.notionStatus')='syncing'").map_err(storage)?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(storage)?;
        rows.map(|r| {
            let (d, v) = r.map_err(storage)?;
            Self::restore(d, v)
        })
        .collect()
    }
    pub(super) fn page_meetings(&self, query: &LibraryQuery) -> Result<Value> {
        let generation = self.generation()?;
        let position = query.position(generation)?;
        let (filter, mut values) = filters(query, false);
        let total: i64 = self
            .db
            .query_row(
                &format!("SELECT count(*) FROM meetings m WHERE {filter}"),
                params_from_iter(values.iter()),
                |r| r.get(0),
            )
            .map_err(storage)?;
        let after = if let Some(p) = position {
            values.extend(p.iter().cloned().map(SqlValue::Text));
            " AND (m.created_at,m.id)<(?,?)"
        } else {
            ""
        };
        let sql = format!("SELECT {HEADER} FROM meetings m WHERE {filter}{after} ORDER BY m.created_at DESC,m.id DESC LIMIT {}",query.limit+1);
        let mut statement = self.db.prepare(&sql).map_err(storage)?;
        let rows = statement
            .query_map(params_from_iter(values), |r| r.get::<_, String>(0))
            .map_err(storage)?;
        let mut meetings: Vec<Value> = rows
            .map(|r| decode(&r.map_err(storage)?))
            .collect::<Result<_>>()?;
        let next = if meetings.len() > query.limit {
            meetings.pop();
            let last = meetings.last().expect("nonempty page");
            query.next_cursor(
                generation,
                vec![
                    last["createdAt"].as_str().unwrap_or("").into(),
                    last["id"].as_str().unwrap_or("").into(),
                ],
            )
        } else {
            Value::Null
        };
        Ok(json!({"meetings":meetings,"nextCursor":next,"total":total}))
    }
    pub(super) fn overview(&self) -> Result<Value> {
        let (count,starred,open):(i64,i64,i64)=self.db.query_row("SELECT count(*),coalesce(sum(CASE WHEN json_extract(document,'$.starred') THEN 1 ELSE 0 END),0),(SELECT count(*) FROM meetings m,json_each(m.document,'$.tasks') t WHERE coalesce(json_extract(t.value,'$.done'),0)=0) FROM meetings",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(storage)?;
        let active:Option<String>=self.db.query_row("SELECT id FROM meetings WHERE json_extract(document,'$.status') IN ('recording','paused','processing') ORDER BY created_at DESC,id DESC LIMIT 1",[],|r|r.get(0)).optional().map_err(storage)?;
        let mut result = json!({"meetingCount":count,"starredCount":starred,"openTaskCount":open,"activeMeetingId":active,"facetsTruncated":{}});
        for field in ["folders", "tags", "people"] {
            let source = if field == "folders" {
                "SELECT DISTINCT json_extract(document,'$.folder') AS value FROM meetings WHERE coalesce(json_extract(document,'$.folder'),'')<>''".to_owned()
            } else {
                format!("SELECT DISTINCT f.value AS value FROM meetings m,json_each(m.document,'$.{field}') f WHERE typeof(f.value)='text'")
            };
            let sql = format!(
                "{source} ORDER BY value COLLATE BINARY LIMIT {}",
                crate::library::FACET_LIMIT + 1
            );
            let mut statement = self.db.prepare(&sql).map_err(storage)?;
            let mut values: Vec<String> = statement
                .query_map([], |r| r.get(0))
                .map_err(storage)?
                .map(|r| r.map_err(storage))
                .collect::<Result<_>>()?;
            result["facetsTruncated"][field] = json!(values.len() > crate::library::FACET_LIMIT);
            values.truncate(crate::library::FACET_LIMIT);
            result[field] = json!(values);
        }
        Ok(result)
    }
    pub(super) fn page_tasks(&self, query: &LibraryQuery) -> Result<Value> {
        let generation = self.generation()?;
        let position = query.position(generation)?;
        let (filter, mut values) = filters(query, true);
        let done = if query.include_done {
            ""
        } else {
            " AND coalesce(json_extract(t.value,'$.done'),0)=0"
        };
        let cte=format!("WITH tasks AS (SELECT json_set(t.value,'$.meetingId',m.id,'$.meetingTitle',json_extract(m.document,'$.title')) AS document,CASE WHEN json_extract(t.value,'$.done') THEN '1' ELSE '0' END AS k0,CASE WHEN coalesce(json_extract(t.value,'$.dueDate'),'')='' THEN '1' ELSE '0' END AS k1,coalesce(json_extract(t.value,'$.dueDate'),'') AS k2,coalesce(json_extract(t.value,'$.createdAt'),'') AS k3,m.id AS k4,json_extract(t.value,'$.id') AS k5 FROM meetings m,json_each(m.document,'$.tasks') t WHERE {filter}{done})");
        let total: i64 = self
            .db
            .query_row(
                &format!("{cte} SELECT count(*) FROM tasks"),
                params_from_iter(values.iter()),
                |r| r.get(0),
            )
            .map_err(storage)?;
        let after = if let Some(p) = position {
            values.extend(p.iter().cloned().map(SqlValue::Text));
            " WHERE (k0,k1,k2,k3,k4,k5)>(?,?,?,?,?,?)"
        } else {
            ""
        };
        let mut statement = self
            .db
            .prepare(&format!(
                "{cte} SELECT document FROM tasks{after} ORDER BY k0,k1,k2,k3,k4,k5 LIMIT {}",
                query.limit + 1
            ))
            .map_err(storage)?;
        let mut tasks: Vec<Value> = statement
            .query_map(params_from_iter(values), |r| r.get::<_, String>(0))
            .map_err(storage)?
            .map(|r| decode(&r.map_err(storage)?))
            .collect::<Result<_>>()?;
        let next = if tasks.len() > query.limit {
            tasks.pop();
            query.next_cursor(
                generation,
                crate::library::task_position(tasks.last().expect("nonempty page")),
            )
        } else {
            Value::Null
        };
        Ok(json!({"tasks":tasks,"total":total,"nextCursor":next}))
    }
}
use rusqlite::OptionalExtension;
