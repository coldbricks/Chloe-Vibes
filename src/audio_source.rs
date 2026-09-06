//! Playback-source selection. A missing explicit choice never captures another device.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioSource {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Default)]
pub struct AudioSources {
    pub available: Vec<AudioSource>,
}

impl AudioSources {
    /// A failed discovery says nothing about endpoint availability. Replace
    /// the last successful catalog only on success, including an empty list.
    pub fn apply_discovery<E>(&mut self, discovered: Result<Vec<AudioSource>, E>) -> Result<(), E> {
        self.available = discovered?;
        Ok(())
    }

    pub fn resolve(&self, selected: Option<&str>) -> Option<&AudioSource> {
        self.available.iter().find(|source| match selected {
            Some(id) => source.id == id,
            None => source.is_default,
        })
    }

    pub fn label(&self, selected: Option<&str>) -> String {
        match (selected, self.resolve(selected)) {
            (None, Some(source)) => format!("Follow Windows default — {}", source.name),
            (None, None) => "Follow Windows default — waiting for audio device".into(),
            (Some(_), Some(source)) => source.name.clone(),
            (Some(_), None) => "Selected device unavailable — waiting to reconnect".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(default: &str, headphones_present: bool) -> AudioSources {
        let mut available = vec![AudioSource {
            id: "speakers".into(),
            name: "Speakers".into(),
            is_default: default == "speakers",
        }];
        if headphones_present {
            available.push(AudioSource {
                id: "headphones".into(),
                name: "Headphones".into(),
                is_default: default == "headphones",
            });
        }
        AudioSources { available }
    }

    #[test]
    fn following_default_moves_to_new_headphones_and_back() {
        assert_eq!(
            catalog("speakers", true).resolve(None).unwrap().id,
            "speakers"
        );
        assert_eq!(
            catalog("headphones", true).resolve(None).unwrap().id,
            "headphones"
        );
        assert_eq!(
            catalog("speakers", false).resolve(None).unwrap().id,
            "speakers"
        );
    }

    #[test]
    fn explicit_source_is_preserved_when_windows_default_changes() {
        assert_eq!(
            catalog("headphones", true)
                .resolve(Some("speakers"))
                .unwrap()
                .id,
            "speakers"
        );
    }

    #[test]
    fn unplugged_explicit_source_waits_and_reconnects_without_fallback() {
        assert!(catalog("speakers", false)
            .resolve(Some("headphones"))
            .is_none());
        assert_eq!(
            catalog("speakers", true)
                .resolve(Some("headphones"))
                .unwrap()
                .id,
            "headphones"
        );
    }

    #[test]
    fn unavailable_labels_do_not_expose_endpoint_ids() {
        assert!(!AudioSources::default()
            .label(Some("private-endpoint-id"))
            .contains("private-endpoint-id"));
        assert!(AudioSources::default().resolve(None).is_none());
    }

    #[test]
    fn discovery_failures_preserve_selection_until_a_successful_refresh() {
        let mut sources = AudioSources::default();
        sources
            .apply_discovery::<&str>(Ok(catalog("speakers", true).available))
            .unwrap();
        for error in ["enumeration interrupted", "discovery still unavailable"] {
            assert_eq!(sources.apply_discovery(Err(error)), Err(error));
            assert_eq!(sources.resolve(None).unwrap().id, "speakers");
            assert_eq!(
                sources.resolve(Some("headphones")).unwrap().id,
                "headphones"
            );
        }
        sources
            .apply_discovery::<&str>(Ok(catalog("headphones", true).available))
            .unwrap();
        assert_eq!(sources.resolve(None).unwrap().id, "headphones");
        assert_eq!(sources.resolve(Some("speakers")).unwrap().id, "speakers");
    }

    #[test]
    fn successful_empty_discovery_removes_devices_and_errors_do_not_resurrect_them() {
        let mut sources = catalog("headphones", true);
        sources.apply_discovery::<&str>(Ok(vec![])).unwrap();
        assert!(sources.resolve(None).is_none());
        assert!(sources.resolve(Some("headphones")).is_none());
        assert!(sources
            .apply_discovery(Err("discovery unavailable"))
            .is_err());
        assert!(sources.resolve(None).is_none());
        assert!(sources.resolve(Some("headphones")).is_none());
        sources
            .apply_discovery::<&str>(Ok(catalog("speakers", false).available))
            .unwrap();
        assert_eq!(sources.resolve(None).unwrap().id, "speakers");
        assert!(sources.resolve(Some("headphones")).is_none());
    }

    #[test]
    fn initial_discovery_failure_keeps_the_waiting_state_without_endpoint_ids() {
        let mut sources = AudioSources::default();
        assert!(sources
            .apply_discovery(Err("discovery unavailable"))
            .is_err());
        assert!(sources.available.is_empty());
        assert!(sources.label(None).contains("waiting"));
        let label = sources.label(Some("private-endpoint-id"));
        assert!(label.contains("waiting"));
        assert!(!label.contains("private-endpoint-id"));
    }
}
