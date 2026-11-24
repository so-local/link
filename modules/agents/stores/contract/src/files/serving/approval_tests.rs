use super::*;
use crate::files::Store;
use link_interactions::{approval::*, sources, *};
use std::sync::atomic::{AtomicBool, Ordering};

struct ReviewedHome {
    objects: Arc<DirStore>,
    interactions: link_interactions::Store,
    writable: AtomicBool,
    scenario: u8,
    request: Mutex<Option<RequestId>>,
}
impl Homes for ReviewedHome {
    fn home(&self, _who: &Identity) -> anyhow::Result<Home> {
        Ok(Home {
            store: self.objects.clone(),
            cap: None,
            write: if self.writable.load(Ordering::SeqCst) {
                vec!["work".into()]
            } else {
                vec![]
            },
        })
    }
    fn reviews_writes(&self, _who: &Identity, _team: Option<&TeamRef>, _path: &str) -> bool {
        true
    }
    fn review_write(
        &self,
        _who: &Identity,
        write: &PreparedWrite,
    ) -> anyhow::Result<Option<sources::ReviewedEffect>> {
        assert_eq!(self.objects.get("home/work/report")?.unwrap().0, b"before");
        let review = PermissionReview {
            capability: "Write the reviewed report".into(),
            targets: vec![write.path.clone()],
            lifetime: "This conditional write, once".into(),
            consequences: vec!["Replace the reviewed version".into()],
            requester_reason: None,
            exact_operation: serde_json::to_string(write)?,
        };
        let requester = sources::requester("files", "service:m1a2i3n4s5v6", "main")?;
        let operation = sources::operation(
            requester.source.clone(),
            &write.payload()?,
            &review,
            Scope::OperationOnce,
            None,
        )?;
        let id = self.interactions.create_bound(
            requester,
            sources::owner(),
            DomainId::new("file.operation").unwrap(),
            sources::confirmation(
                "Report write".into(),
                "Write this report?".into(),
                operation.id.to_string(),
                std::time::Duration::from_secs(30),
            ),
            Effect::Authorize(Box::new(operation)),
            Some(review),
            now(),
        )?;
        *self.request.lock().unwrap() = Some(id.clone());
        let e = self.interactions.plan_delivery(
            &id,
            "cli:main",
            "cli",
            &render::text_capabilities(),
            None,
            now(),
        )?;
        if self.scenario == 1 {
            self.objects
                .put("home/work/report", b"other writer", &Expect::Any)?;
        }
        if self.scenario == 2 {
            self.writable.store(false, Ordering::SeqCst);
        }
        self.interactions.submit(
            &Ingress {
                actor: sources::owner(),
                route: "cli:main".into(),
                surface: "cli".into(),
                input_kind: InputKind::Control,
            },
            ResponseCandidate {
                request: id.clone(),
                revision: 1,
                delivery: e.delivery.id,
                value: reply::parse(&e.question, "yes", false, false, false)?,
                transport_event: TransportEventId::new("owner-yes").unwrap(),
            },
            now(),
        )?;
        self.interactions
            .reviewed_effect(&id, &SourceId::new("files").unwrap())
            .map(Some)
    }
}

#[test]
fn a_reviewed_object_write_rechecks_version_and_grants_at_the_commit_boundary() {
    for scenario in 0..=2 {
        let dir = link_test_kit::TempDir::new("reviewed-object-write");
        let objects = Arc::new(DirStore::new(&dir.join("objects")).unwrap());
        objects
            .put("home/work/report", b"before", &Expect::Absent)
            .unwrap();
        let homes = Arc::new(ReviewedHome {
            objects: objects.clone(),
            interactions: link_interactions::Store::at(dir.join("interactions")),
            writable: AtomicBool::new(true),
            scenario,
            request: Mutex::new(None),
        });
        let who = Identity {
            agent: "aaaaaaaaaaa1".into(),
            service: String::new(),
            owner: "me@box".into(),
            teams: vec![],
        };
        let keys = Arc::new(crate::bridge::Keyring::at(&dir.join("keys")));
        keys.bind(
            &who,
            &link_secretive::Secret::from("0123456789abcdef0123456789abcdef".to_string()),
        )
        .unwrap();
        let bridge = Bridge::new(keys, homes.clone());
        let data = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            b"reviewed report",
        );
        let result = bridge.act(
            &who,
            Action::Write {
                team: None,
                path: "work/report".into(),
                data,
                expect: Expect::Any,
            },
        );
        let id = homes.request.lock().unwrap().clone().unwrap();
        let effect = homes.interactions.snapshot(&id, now()).unwrap().effect;
        let bytes = objects.get("home/work/report").unwrap().unwrap().0;
        match scenario {
            0 => {
                assert!(result.is_ok());
                assert_eq!(bytes, b"reviewed report");
                assert!(matches!(effect, EffectState::Applied { .. }));
            }
            1 => {
                assert!(is_conflict(&result.unwrap_err()));
                assert_eq!(bytes, b"other writer");
                assert!(matches!(effect, EffectState::Failed { .. }));
            }
            _ => {
                assert!(result.is_err());
                assert_eq!(bytes, b"before");
                assert!(matches!(effect, EffectState::Revoked { .. }));
            }
        }
    }
}
