use serde::{Deserialize, Serialize};
use smad_protocol::{DocumentKind, ProjectKey, ProjectMetadata};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentKey {
    pub project: ProjectKey,
    pub canonical_source_file: String,
    #[serde(default)]
    pub kind: DocumentKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentLifecycle {
    Dormant,
    Loading,
    Ready,
    Failed,
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub key: DocumentKey,
    #[serde(default)]
    pub kind: DocumentKind,
    pub lavish_url: String,
    pub title: Option<String>,
    pub lifecycle: DocumentLifecycle,
    pub last_activated_at: u64,
    pub load_error: Option<String>,
    #[serde(skip)]
    pub open_request_id: Option<String>,
    #[serde(default)]
    pub highlights: Vec<smad_protocol::LineRange>,
    #[serde(default)]
    pub old_highlights: Vec<smad_protocol::LineRange>,
    #[serde(default)]
    pub comparison: Option<crate::snapshots::ComparisonSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub key: ProjectKey,
    pub label: String,
    pub raw_tab_name: Option<String>,
    pub documents: Vec<Document>,
    pub selected_document: Option<String>,
    #[serde(default)]
    pub selected_kind: DocumentKind,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Project {
    pub fn selected(&self) -> Option<&Document> {
        self.documents.iter().find(|document| {
            Some(&document.key.canonical_source_file) == self.selected_document.as_ref()
                && document.kind == self.selected_kind
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BrowserModel {
    /// Projects remain in creation order.
    pub projects: Vec<Project>,
    pub selected_project: Option<ProjectKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenOutcome {
    pub project_created: bool,
    pub document_created: bool,
    pub refreshed_documents: usize,
}

impl BrowserModel {
    pub fn open_url(
        &mut self,
        project: ProjectMetadata,
        canonical_source_file: impl Into<String>,
        lavish_url: impl Into<String>,
        activated_at: u64,
    ) -> OpenOutcome {
        self.open_document(
            project,
            canonical_source_file.into(),
            lavish_url.into(),
            DocumentKind::LavishReview,
            activated_at,
        )
    }

    pub fn open_markdown(
        &mut self,
        project: ProjectMetadata,
        source: String,
        activated_at: u64,
    ) -> OpenOutcome {
        self.open_document(
            project,
            source,
            String::new(),
            DocumentKind::Markdown,
            activated_at,
        )
    }

    pub fn open_document(
        &mut self,
        project: ProjectMetadata,
        source: String,
        url: String,
        kind: DocumentKind,
        activated_at: u64,
    ) -> OpenOutcome {
        let mut refreshed_documents = 0;
        for existing_project in &mut self.projects {
            for document in &mut existing_project.documents {
                if document.key.canonical_source_file == source
                    && document.kind == kind
                    && document.lavish_url != url
                {
                    document.lavish_url.clone_from(&url);
                    document.lifecycle = DocumentLifecycle::Dormant;
                    document.load_error = None;
                    refreshed_documents += 1;
                }
            }
        }

        let project_index = self
            .projects
            .iter()
            .position(|item| item.key == project.key);
        let project_created = project_index.is_none();
        let project_index = project_index.unwrap_or_else(|| {
            self.projects.push(Project {
                key: project.key.clone(),
                label: project.label.clone(),
                raw_tab_name: project.raw_tab_name.clone(),
                documents: Vec::new(),
                selected_document: None,
                selected_kind: kind,
                created_at: activated_at,
                updated_at: activated_at,
            });
            self.projects.len() - 1
        });

        let target = &mut self.projects[project_index];
        target.label = project.label;
        target.raw_tab_name = project.raw_tab_name;
        target.updated_at = activated_at;
        let document_index = target.documents.iter().position(|document| {
            document.key.canonical_source_file == source && document.kind == kind
        });
        let document_created = document_index.is_none();
        if let Some(index) = document_index {
            target.documents[index].last_activated_at = activated_at;
        } else {
            target.documents.push(Document {
                key: DocumentKey {
                    project: target.key.clone(),
                    canonical_source_file: source.clone(),
                    kind,
                },
                kind,
                lavish_url: url,
                title: None,
                lifecycle: DocumentLifecycle::Dormant,
                last_activated_at: activated_at,
                load_error: None,
                open_request_id: None,
                highlights: Vec::new(),
                old_highlights: Vec::new(),
                comparison: None,
            });
        }
        target.selected_document = Some(source);
        target.selected_kind = kind;
        self.selected_project = Some(target.key.clone());

        OpenOutcome {
            project_created,
            document_created,
            refreshed_documents,
        }
    }

    pub fn select_project(&mut self, key: &ProjectKey, activated_at: u64) -> bool {
        let Some(project) = self.projects.iter_mut().find(|project| &project.key == key) else {
            return false;
        };
        project.updated_at = activated_at;
        if let Some(source) = &project.selected_document
            && let Some(document) = project.documents.iter_mut().find(|document| {
                &document.key.canonical_source_file == source
                    && document.kind == project.selected_kind
            })
        {
            document.last_activated_at = activated_at;
        }
        self.selected_project = Some(key.clone());
        true
    }

    /// Update display metadata without touching identity, selection, or LRU state.
    pub fn rename_project(&mut self, key: &ProjectKey, label: String, raw_name: String) -> bool {
        let Some(project) = self.projects.iter_mut().find(|project| &project.key == key) else {
            return false;
        };
        if project.label == label && project.raw_tab_name.as_deref() == Some(&raw_name) {
            return false;
        }
        project.label = label;
        project.raw_tab_name = Some(raw_name);
        true
    }

    pub fn close_project(&mut self, key: &ProjectKey) -> bool {
        let Some(index) = self.projects.iter().position(|project| &project.key == key) else {
            return false;
        };
        let was_selected = self.selected_project.as_ref() == Some(key);
        self.projects.remove(index);
        if was_selected {
            self.selected_project = self
                .projects
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|prior| self.projects.get(prior))
                })
                .map(|project| project.key.clone());
        }
        true
    }

    pub fn select_document(&mut self, key: &DocumentKey, activated_at: u64) -> bool {
        let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| project.key == key.project)
        else {
            return false;
        };
        let Some(document) = project
            .documents
            .iter_mut()
            .find(|document| document.key == *key)
        else {
            return false;
        };
        document.last_activated_at = activated_at;
        project.updated_at = activated_at;
        project.selected_document = Some(key.canonical_source_file.clone());
        project.selected_kind = key.kind;
        self.selected_project = Some(key.project.clone());
        true
    }

    pub fn close_document(&mut self, key: &DocumentKey) -> bool {
        let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| project.key == key.project)
        else {
            return false;
        };
        let Some(index) = project
            .documents
            .iter()
            .position(|document| document.key == *key)
        else {
            return false;
        };
        let was_selected = project.selected_document.as_deref() == Some(&key.canonical_source_file)
            && project.selected_kind == key.kind;
        project.documents.remove(index);
        if was_selected {
            project.selected_document = project
                .documents
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|prior| project.documents.get(prior))
                })
                .map(|document| {
                    project.selected_kind = document.kind;
                    document.key.canonical_source_file.clone()
                });
        }
        true
    }

    pub fn set_document_lifecycle(
        &mut self,
        key: &DocumentKey,
        lifecycle: DocumentLifecycle,
        load_error: Option<String>,
    ) -> bool {
        let Some(document) = self
            .projects
            .iter_mut()
            .flat_map(|project| &mut project.documents)
            .find(|document| document.key == *key)
        else {
            return false;
        };
        document.lifecycle = lifecycle;
        document.load_error = load_error;
        true
    }

    pub fn inactive_documents_lru(&self) -> Vec<&Document> {
        let active = self.selected_project.as_ref().and_then(|project_key| {
            self.projects
                .iter()
                .find(|project| &project.key == project_key)
                .and_then(|project| {
                    project
                        .selected_document
                        .as_ref()
                        .map(|source| DocumentKey {
                            project: project.key.clone(),
                            canonical_source_file: source.clone(),
                            kind: project.selected_kind,
                        })
                })
        });
        let mut documents = self
            .projects
            .iter()
            .flat_map(|project| &project.documents)
            .filter(|document| active.as_ref() != Some(&document.key))
            .collect::<Vec<_>>();
        documents.sort_by_key(|document| document.last_activated_at);
        documents
    }
}
