//! Conservative identity suggestions. Only human-confirmed examples teach profiles.
use std::collections::{HashMap, HashSet};
use std::io;

use libsql::{Connection, TransactionBehavior, params};
use serde::Serialize;
use utoipa::ToSchema;

use crate::processing::{ProcessingQueue, active_pipeline_version};

const SOURCE: &str = "centroid-v1";
const MIN_EXAMPLES: usize = 5;
const MIN_DAYS: usize = 3;
const MIN_SCORE: f64 = 0.65;
const MIN_MARGIN: f64 = 0.15;

#[derive(Default, Serialize, ToSchema)]
pub struct MatchingSummary {
    pub examined: u64,
    pub proposed: u64,
}

#[derive(Default)]
struct Profile {
    sum: Vec<f64>,
    photos: HashSet<String>,
    days: HashSet<String>,
}

fn normalize(mut values: Vec<f64>) -> Option<Vec<f64>> {
    let norm = values.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !norm.is_finite() || norm <= 1e-12 {
        return None;
    }
    values.iter_mut().for_each(|v| *v /= norm);
    Some(values)
}

fn decode(bytes: Vec<u8>, dimension: i64) -> Option<Vec<f64>> {
    if dimension <= 0 || dimension > 4096 || bytes.len() != dimension as usize * 4 {
        return None;
    }
    normalize(
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()) as f64)
            .collect(),
    )
}

type ProfileKey = (String, String, usize); // person, model, dimension

fn best_match(
    profiles: &HashMap<ProfileKey, Profile>,
    model: &str,
    embedding: &[f64],
) -> Option<(String, f64)> {
    let mut scores = profiles
        .iter()
        .filter_map(|((person, candidate_model, dimension), profile)| {
            if candidate_model != model || *dimension != embedding.len() {
                return None;
            }
            let centroid = normalize(profile.sum.clone())?;
            let score = centroid
                .iter()
                .zip(embedding)
                .map(|(a, b)| a * b)
                .sum::<f64>();
            Some((person, profile, score))
        })
        .collect::<Vec<_>>();
    scores.sort_by(|a, b| b.2.total_cmp(&a.2));
    let (person, profile, score) = scores.first()?;
    // Even unenrolled people compete, preventing their faces from being forced
    // into an enrolled person's bucket.
    let runner_up = scores.get(1).map_or(-1.0, |entry| entry.2);
    if profile.photos.len() < MIN_EXAMPLES
        || profile.days.len() < MIN_DAYS
        || *score < MIN_SCORE
        || score - runner_up < MIN_MARGIN
    {
        return None;
    }
    Some(((*person).clone(), score.clamp(-1.0, 1.0)))
}

/// Call within a write transaction so label edits cannot race profile creation.
pub(crate) async fn propose(
    connection: &Connection,
    pipeline: &str,
    photo: Option<&str>,
) -> io::Result<MatchingSummary> {
    let mut rows = connection
        .query(
            "SELECT f.person_id, f.embedding_model, f.embedding_dimension, f.embedding,
                f.photo_id, substr(p.captured_at, 1, 10)
         FROM faces f JOIN photos p ON p.id = f.photo_id
         WHERE f.pipeline_version = ?1 AND f.identity_state = 'confirmed'
           AND f.identity_source = 'manual' AND f.person_id IS NOT NULL
         ORDER BY f.id",
            params![pipeline],
        )
        .await
        .map_err(io::Error::other)?;
    let mut profiles: HashMap<ProfileKey, Profile> = HashMap::new();
    while let Some(row) = rows.next().await.map_err(io::Error::other)? {
        let dimension: i64 = row.get(2).map_err(io::Error::other)?;
        let Some(vector) = decode(row.get(3).map_err(io::Error::other)?, dimension) else {
            continue;
        };
        let profile = profiles
            .entry((
                row.get(0).map_err(io::Error::other)?,
                row.get(1).map_err(io::Error::other)?,
                vector.len(),
            ))
            .or_default();
        // A repeated face in one image must not count as another enrollment photo.
        if !profile.photos.insert(row.get(4).map_err(io::Error::other)?) {
            continue;
        }
        profile.days.insert(row.get(5).map_err(io::Error::other)?);
        if profile.sum.is_empty() {
            profile.sum = vec![0.0; vector.len()];
        }
        for (sum, value) in profile.sum.iter_mut().zip(vector) {
            *sum += value;
        }
    }
    drop(rows);
    let mut rows = connection
        .query(
            "SELECT id, embedding_model, embedding_dimension, embedding FROM faces
         WHERE pipeline_version = ?1 AND (?2 IS NULL OR photo_id = ?2)
           AND (identity_state = 'proposed' OR
                (identity_state = 'unknown' AND identity_source IS NULL))",
            params![pipeline, photo],
        )
        .await
        .map_err(io::Error::other)?;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next().await.map_err(io::Error::other)? {
        let id: String = row.get(0).map_err(io::Error::other)?;
        let model: String = row.get(1).map_err(io::Error::other)?;
        let vector = decode(
            row.get(3).map_err(io::Error::other)?,
            row.get(2).map_err(io::Error::other)?,
        );
        let matched = vector.and_then(|v| best_match(&profiles, &model, &v));
        candidates.push((id, matched));
    }
    drop(rows);
    let mut summary = MatchingSummary::default();
    for (id, matched) in candidates {
        summary.examined += 1;
        let (person, score) = match matched {
            Some((person, score)) => {
                summary.proposed += 1;
                (Some(person), Some(score))
            }
            None => (None, None),
        };
        connection
            .execute(
                "UPDATE faces SET person_id = ?2,
               identity_state = CASE WHEN ?2 IS NULL THEN 'unknown' ELSE 'proposed' END,
               identity_source = CASE WHEN ?2 IS NULL THEN NULL ELSE ?4 END,
               identity_score = ?3, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                params![id, person, score, SOURCE],
            )
            .await
            .map_err(io::Error::other)?;
    }
    Ok(summary)
}

impl ProcessingQueue {
    pub async fn refresh_face_suggestions(&self) -> io::Result<MatchingSummary> {
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(io::Error::other)?;
        let summary = propose(&transaction, &active_pipeline_version()?, None).await?;
        transaction.commit().await.map_err(io::Error::other)?;
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(vector: Vec<f64>, count: usize, days: usize) -> Profile {
        Profile {
            sum: vector,
            photos: (0..count).map(|i| i.to_string()).collect(),
            days: (0..days).map(|i| i.to_string()).collect(),
        }
    }

    #[test]
    fn enrollment_competitors_and_model_compatibility_gate_suggestions() {
        let key = ("alex".to_owned(), "sface".to_owned(), 2);
        let mut profiles = HashMap::from([(key.clone(), profile(vec![1.0, 0.0], 4, 3))]);
        assert!(best_match(&profiles, "sface", &[1.0, 0.0]).is_none());
        profiles.insert(key.clone(), profile(vec![1.0, 0.0], 5, 2));
        assert!(best_match(&profiles, "sface", &[1.0, 0.0]).is_none());
        profiles.insert(key, profile(vec![1.0, 0.0], 5, 3));
        assert_eq!(
            best_match(&profiles, "sface", &[1.0, 0.0]).unwrap().0,
            "alex"
        );
        assert!(best_match(&profiles, "other-model", &[1.0, 0.0]).is_none());
        assert!(best_match(&profiles, "sface", &[0.0, 1.0]).is_none());
        profiles.insert(
            ("visitor".into(), "sface".into(), 2),
            profile(vec![0.99, 0.01], 1, 1),
        );
        assert!(best_match(&profiles, "sface", &[1.0, 0.0]).is_none());
        assert!(decode(vec![0; 8], 2).is_none());
        assert!(decode(vec![0; 7], 2).is_none());
        assert!(decode(f32::NAN.to_le_bytes().to_vec(), 1).is_none());
    }

    #[tokio::test]
    async fn suggestions_preserve_manual_labels_and_rejections_and_never_teach_themselves() {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let connection = db.connect().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE photos (id TEXT PRIMARY KEY, captured_at TEXT);
            CREATE TABLE faces (id TEXT PRIMARY KEY, photo_id TEXT, pipeline_version TEXT,
            embedding_model TEXT, embedding_dimension INTEGER, embedding BLOB, person_id TEXT,
            identity_state TEXT, identity_source TEXT, identity_score REAL, updated_at TEXT);",
            )
            .await
            .unwrap();
        for i in 0..8 {
            let id = i.to_string();
            connection
                .execute(
                    "INSERT INTO photos VALUES (?1, ?2)",
                    params![id.clone(), format!("2026-09-{:02}", i + 1)],
                )
                .await
                .unwrap();
            let (person, state, source) = if i < 4 {
                (Some("alex"), "confirmed", Some("manual"))
            } else if i == 4 {
                (Some("alex"), "proposed", Some(SOURCE))
            } else if i == 7 {
                (None, "unknown", Some("manual-rejected"))
            } else {
                (None, "unknown", None)
            };
            connection.execute("INSERT INTO faces VALUES (?1, ?1, 'v1', 'sface', 2, ?2, ?3, ?4, ?5, NULL, NULL)",
                params![id, [1.0_f32.to_le_bytes(), 0.0_f32.to_le_bytes()].concat(), person, state, source]).await.unwrap();
        }
        assert_eq!(propose(&connection, "v1", None).await.unwrap().proposed, 0);
        connection.execute("UPDATE faces SET person_id='alex', identity_state='confirmed', identity_source='manual' WHERE id='4'", ()).await.unwrap();
        assert_eq!(
            propose(&connection, "v1", Some("5"))
                .await
                .unwrap()
                .proposed,
            1
        );
        assert_eq!(propose(&connection, "v1", None).await.unwrap().proposed, 2);
        assert_eq!(propose(&connection, "v1", None).await.unwrap().proposed, 2);
        let mut rows = connection
            .query(
                "SELECT identity_state, identity_source FROM faces WHERE id='7'",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<String>(0).unwrap(), "unknown");
        assert_eq!(row.get::<String>(1).unwrap(), "manual-rejected");
        connection.execute("UPDATE faces SET person_id=NULL, identity_state='unknown', identity_source='manual-rejected' WHERE id='4'", ()).await.unwrap();
        assert_eq!(propose(&connection, "v1", None).await.unwrap().proposed, 0);
        let mut rows = connection.query("SELECT COUNT(*) FROM faces WHERE identity_state='confirmed' AND identity_source='manual'", ()).await.unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            4
        );
    }
}
