//! Local voice identities. Matching is gated; ambiguity never creates a match.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tuck_core::{ports::Result, AppError};
const MATCH: f32 = 0.85;
const MARGIN: f32 = 0.15;
const MIN_ENROLL_SECONDS: f64 = 15.;
const SHORT_MATCH: f32 = 0.45;
const SHORT_SUPPORT: f32 = 0.40;
const SHORT_MARGIN: f32 = 0.25;
#[derive(Clone, Serialize, Deserialize)]
struct Profile {
    id: String,
    name: String,
    #[serde(default)]
    color: u8,
    embedding: Vec<f32>,
}
#[derive(Default, Serialize, Deserialize)]
struct Database {
    revision: String,
    profiles: Vec<Profile>,
}
pub struct VoiceMemory {
    path: PathBuf,
    gate: Mutex<()>,
}
fn error() -> AppError {
    AppError::new(
        "voice_memory_error",
        "Stored voice profiles are unavailable. Speech will remain unidentified.",
    )
}
pub fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != 256 || b.len() != 256 || a.iter().chain(b).any(|v| !v.is_finite()) {
        return None;
    }
    let norms =
        (a.iter().map(|v| v * v).sum::<f32>() * b.iter().map(|v| v * v).sum::<f32>()).sqrt();
    if !norms.is_finite() || norms <= 1e-8 {
        return None;
    }
    let score = a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() / norms;
    score.is_finite().then_some(score)
}
pub struct VoiceMatcher {
    profiles: Vec<Profile>,
}
impl VoiceMatcher {
    pub fn identify(&self, embedding: &[f32]) -> Option<(String, String)> {
        VoiceMemory::best(&self.profiles, embedding).map(|p| (p.id.clone(), p.name.clone()))
    }
    /// A short window can only verify a context-selected identity, never choose one.
    pub fn supports(&self, id: &str, embedding: &[f32]) -> bool {
        let Some(profile) = self.profiles.iter().find(|p| p.id == id) else {
            return false;
        };
        let Some(score) = cosine(&profile.embedding, embedding) else {
            return false;
        };
        let runner = self
            .profiles
            .iter()
            .filter(|p| p.id != id)
            .filter_map(|p| cosine(&p.embedding, embedding))
            .fold(-1., f32::max);
        score >= 0.70 && score - runner >= MARGIN
    }
    // A separate, duration-limited final-pass policy. It does not relax the
    // whole-profile gate or learn from short queries. Both boundary probes must
    // agree, including a wide separation from every stored competing voice.
    pub fn identify_short(&self, probes: &[Vec<f32>]) -> Option<String> {
        self.short_consensus(probes, SHORT_MATCH)
    }
    pub fn supports_short(&self, id: &str, probes: &[Vec<f32>]) -> bool {
        self.short_consensus(probes, SHORT_SUPPORT).as_deref() == Some(id)
    }
    fn short_consensus(&self, probes: &[Vec<f32>], floor: f32) -> Option<String> {
        if probes.len() != 2 || self.profiles.len() < 2 {
            return None;
        }
        let mut identity: Option<String> = None;
        for probe in probes {
            let mut scores = self
                .profiles
                .iter()
                .filter_map(|p| cosine(&p.embedding, probe).map(|s| (s, p)))
                .collect::<Vec<_>>();
            scores.sort_by(|a, b| b.0.total_cmp(&a.0));
            let (score, profile) = scores.first()?;
            let runner = scores.get(1).map(|s| s.0).unwrap_or(-1.);
            if *score < floor
                || score - runner < SHORT_MARGIN
                || identity.as_ref().is_some_and(|id| id != &profile.id)
            {
                return None;
            }
            identity = Some(profile.id.clone());
        }
        identity
    }
    pub fn names(&self) -> Vec<Value> {
        self.profiles
            .iter()
            .map(|p| json!({"id":p.id,"name":p.name,"color":p.color}))
            .collect()
    }
}
impl VoiceMemory {
    pub fn new(root: &Path) -> Self {
        Self {
            path: root.join("voice-memory.json"),
            gate: Mutex::new(()),
        }
    }
    fn read(&self) -> Result<Database> {
        if !self.path.exists() {
            return Ok(Database {
                revision: super::models::diarization_manifest()?.revision,
                profiles: vec![],
            });
        }
        let meta = fs::symlink_metadata(&self.path).map_err(|_| error())?;
        if meta.file_type().is_symlink() || meta.len() > 4 * 1024 * 1024 {
            return Err(error());
        }
        let db: Database = serde_json::from_slice(&fs::read(&self.path).map_err(|_| error())?)
            .map_err(|_| error())?;
        if db.revision != super::models::diarization_manifest()?.revision || db.profiles.len() > 256
        {
            return Err(error());
        }
        if db
            .profiles
            .iter()
            .any(|p| cosine(&p.embedding, &p.embedding).is_none())
        {
            return Err(error());
        }
        Ok(db)
    }
    fn write(&self, db: &Database) -> Result<()> {
        let root = self.path.parent().ok_or_else(error)?;
        fs::create_dir_all(root).map_err(|_| error())?;
        let mut stage = tempfile::NamedTempFile::new_in(root).map_err(|_| error())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            stage
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| error())?;
        }
        stage
            .write_all(&serde_json::to_vec(db).map_err(|_| error())?)
            .map_err(|_| error())?;
        stage.as_file().sync_all().map_err(|_| error())?;
        stage.persist(&self.path).map_err(|_| error())?;
        Ok(())
    }
    fn best<'a>(profiles: &'a [Profile], embedding: &[f32]) -> Option<&'a Profile> {
        let mut scores = profiles
            .iter()
            .filter_map(|p| cosine(&p.embedding, embedding).map(|s| (p, s)))
            .collect::<Vec<_>>();
        scores.sort_by(|a, b| b.1.total_cmp(&a.1));
        let (profile, score) = scores.first()?;
        let runner = scores.get(1).map(|s| s.1).unwrap_or(-1.);
        (*score >= MATCH && score - runner >= MARGIN).then_some(*profile)
    }
    pub fn matcher(&self) -> Result<VoiceMatcher> {
        let _gate = self.gate.lock().map_err(|_| error())?;
        Ok(VoiceMatcher {
            profiles: self.read()?.profiles,
        })
    }
    pub fn forget(&self) -> Result<()> {
        let _gate = self.gate.lock().map_err(|_| error())?;
        self.write(&Database {
            revision: super::models::diarization_manifest()?.revision,
            profiles: vec![],
        })
    }
    pub fn identify(&self, embedding: &[f32]) -> Result<Option<(String, String)>> {
        let _gate = self.gate.lock().map_err(|_| error())?;
        let db = self.read()?;
        Ok(Self::best(&db.profiles, embedding).map(|p| (p.id.clone(), p.name.clone())))
    }
    pub fn enroll_or_match(
        &self,
        embedding: &[f32],
        seconds: f64,
    ) -> Result<Option<(String, String)>> {
        let _gate = self.gate.lock().map_err(|_| error())?;
        let mut db = self.read()?;
        if let Some(p) = Self::best(&db.profiles, embedding) {
            return Ok(Some((p.id.clone(), p.name.clone())));
        }
        // Do not convert an ambiguous near-match into another identity.
        let closest = db
            .profiles
            .iter()
            .filter_map(|p| cosine(&p.embedding, embedding))
            .fold(-1., f32::max);
        if seconds < MIN_ENROLL_SECONDS
            || closest >= 0.5
            || db.profiles.len() >= 256
            || cosine(embedding, embedding).is_none()
        {
            return Ok(None);
        }
        let profile = Profile {
            id: format!("voice-{}", uuid::Uuid::new_v4()),
            name: format!("Voice {}", db.profiles.len() + 1),
            color: (db.profiles.len() % 6) as u8,
            embedding: embedding.to_vec(),
        };
        let output = (profile.id.clone(), profile.name.clone());
        db.profiles.push(profile);
        self.write(&db)?;
        Ok(Some(output))
    }
    pub fn names(&self) -> Result<Vec<Value>> {
        let _gate = self.gate.lock().map_err(|_| error())?;
        Ok(self
            .read()?
            .profiles
            .into_iter()
            .map(|p| json!({"id":p.id,"name":p.name,"color":p.color}))
            .collect())
    }
    pub fn rename(&self, id: &str, name: &str) -> Result<()> {
        let _gate = self.gate.lock().map_err(|_| error())?;
        let mut db = self.read()?;
        if let Some(p) = db.profiles.iter_mut().find(|p| p.id == id) {
            p.name = name.to_owned();
            self.write(&db)
        } else {
            // Imported transcripts can retain a voice ID without its local
            // biometric template. Renaming text must not invent a template.
            Ok(())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn vector(axis: usize) -> Vec<f32> {
        let mut v = vec![0.; 256];
        v[axis] = 1.;
        v
    }
    #[test]
    fn stable_identity_survives_reload_and_unknown_does_not_guess() {
        let root = tempfile::tempdir().unwrap();
        let memory = VoiceMemory::new(root.path());
        let a = memory.enroll_or_match(&vector(0), 20.).unwrap().unwrap();
        memory.rename(&a.0, "Confirmed name").unwrap();
        let reopened = VoiceMemory::new(root.path());
        assert_eq!(
            reopened.identify(&vector(0)).unwrap().unwrap(),
            (a.0, "Confirmed name".into())
        );
        assert!(reopened.identify(&vector(1)).unwrap().is_none());
        assert!(reopened.enroll_or_match(&vector(1), 1.).unwrap().is_none());
    }
    #[test]
    fn short_queries_require_two_separated_consistent_probes_without_relaxing_profile_match() {
        let a = Profile {
            id: "a".into(),
            name: "A".into(),
            color: 0,
            embedding: vector(0),
        };
        let b = Profile {
            id: "b".into(),
            name: "B".into(),
            color: 1,
            embedding: vector(1),
        };
        let matcher = VoiceMatcher {
            profiles: vec![a, b.clone()],
        };
        let probe = |x: f32, y: f32| {
            let mut v = vector(2);
            v[0] = x;
            v[1] = y;
            v[2] = (1. - x * x - y * y).sqrt();
            v
        };
        let queries = vec![probe(0.48, 0.1), probe(0.49, 0.1)];
        assert_eq!(matcher.identify_short(&queries).as_deref(), Some("a"));
        assert!(
            matcher.identify(&queries[0]).is_none(),
            "long-profile threshold stays strict"
        );
        assert!(matcher.identify_short(&queries[..1]).is_none());
        assert!(matcher
            .identify_short(&[probe(0.48, 0.4), probe(0.49, 0.4)])
            .is_none());
        assert!(matcher
            .identify_short(&[queries[0].clone(), probe(0.1, 0.49)])
            .is_none());
        assert!(
            VoiceMatcher { profiles: vec![b] }
                .identify_short(&queries)
                .is_none(),
            "missing true voice must abstain"
        );
        let support = vec![probe(0.42, 0.1), probe(0.43, 0.1)];
        assert!(matcher.identify_short(&support).is_none());
        assert!(matcher.supports_short("a", &support));
        assert!(!matcher.supports_short("b", &support));
        assert!(matcher
            .identify_short(&[vec![f32::NAN; 256], queries[0].clone()])
            .is_none());
    }
    #[test]
    fn ambiguous_profiles_and_invalid_vectors_are_rejected() {
        let a = Profile {
            id: "a".into(),
            name: "A".into(),
            color: 0,
            embedding: vector(0),
        };
        let mut b = a.clone();
        b.id = "b".into();
        assert!(VoiceMemory::best(&[a, b], &vector(0)).is_none());
        assert!(cosine(&[f32::NAN; 256], &vector(0)).is_none());
    }
}
