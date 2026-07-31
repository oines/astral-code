//! Model catalog behavior for the Grok-style `/model` argument picker.

use codex_app_server_protocol::Model;
use codex_app_server_protocol::ReasoningEffortOption;
use codex_protocol::openai_models::ReasoningEffort;
use std::collections::HashSet;

use crate::slash::fuzzy_match;

const LEGACY_REASONING_EFFORTS: &[(ReasoningEffort, &str)] = &[
    (ReasoningEffort::XHigh, "Extended reasoning"),
    (ReasoningEffort::High, "Heavy reasoning"),
    (ReasoningEffort::Medium, "Balanced reasoning"),
    (ReasoningEffort::Low, "Faster, lighter reasoning"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelSelection {
    pub(crate) model: String,
    pub(crate) model_provider: String,
    pub(crate) display_name: String,
    pub(crate) effort: ReasoningEffort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelSuggestion {
    pub(crate) display: String,
    pub(crate) description: String,
    pub(crate) insert_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelResolveError {
    UnknownModel(String),
    UnsupportedEffort { model: String, effort: String },
}

impl std::fmt::Display for ModelResolveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownModel(model) => write!(formatter, "Unknown model: {model}"),
            Self::UnsupportedEffort { model, effort } => {
                write!(
                    formatter,
                    "{model} does not support reasoning effort {effort}"
                )
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ModelCatalog {
    models: Vec<Model>,
    current_model: String,
    current_provider: String,
    current_effort: Option<ReasoningEffort>,
    observed_reasoning_models: HashSet<(String, String)>,
}

impl ModelCatalog {
    pub(crate) fn replace(
        &mut self,
        models: Vec<Model>,
        current_model: impl Into<String>,
        current_provider: impl Into<String>,
        current_effort: Option<ReasoningEffort>,
    ) {
        self.models = models;
        self.current_model = current_model.into();
        self.current_provider = current_provider.into();
        self.current_effort = current_effort;
        let available_models = self
            .models
            .iter()
            .map(model_identity)
            .collect::<HashSet<_>>();
        self.observed_reasoning_models
            .retain(|identity| available_models.contains(identity));
        self.remember_current_reasoning_model();
    }

    pub(crate) fn update_current(
        &mut self,
        model: impl Into<String>,
        model_provider: impl Into<String>,
        effort: Option<ReasoningEffort>,
    ) {
        self.current_model = model.into();
        self.current_provider = model_provider.into();
        self.current_effort = effort;
        self.remember_current_reasoning_model();
    }

    pub(crate) fn suggestions(&self, args_query: &str) -> Vec<ModelSuggestion> {
        if let Some((model, effort_query)) = self.effort_phase(args_query) {
            return self.effort_suggestions_for(model, effort_query, |effort| {
                format!("/model {} {effort}", model.display_name)
            });
        }

        let query = args_query.trim();
        let mut ranked = self
            .models
            .iter()
            .enumerate()
            .filter_map(|(order, model)| {
                model_match(model, query).map(|score| {
                    let current = self.is_current(model);
                    (model, score, current, order)
                })
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| b.1.cmp(&a.1))
                .then(a.3.cmp(&b.3))
        });
        ranked
            .into_iter()
            .map(|(model, _, current, _)| ModelSuggestion {
                display: if current {
                    format!("{} (current)", model.display_name)
                } else {
                    model.display_name.clone()
                },
                description: model.description.clone(),
                insert_text: format!("/model {} ", model.display_name),
            })
            .collect()
    }

    pub(crate) fn effort_suggestions(&self, query: &str) -> Vec<ModelSuggestion> {
        self.current_model()
            .map(|model| {
                self.effort_suggestions_for(model, query, |effort| format!("/effort {effort}"))
            })
            .unwrap_or_default()
    }

    pub(crate) fn is_effort_phase(&self, args_query: &str) -> bool {
        self.effort_phase(args_query).is_some()
    }

    pub(crate) fn is_complete_selection(&self, args: &str) -> bool {
        if let Some((_, effort)) = self.effort_phase(args) {
            return !effort.is_empty() && self.resolve(args).is_ok();
        }
        false
    }

    pub(crate) fn resolve(&self, args: &str) -> Result<ModelSelection, ModelResolveError> {
        let args = args.trim();
        if let Some(model) = self
            .models
            .iter()
            .find(|model| model_name_matches(model, args))
        {
            return Ok(selection(model, model.default_reasoning_effort.clone()));
        }
        let mut candidates = self
            .models
            .iter()
            .flat_map(|model| model_names(model).map(move |name| (model, name)))
            .filter(|(_, name)| {
                args.get(..name.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(name))
                    && args[name.len()..].starts_with(char::is_whitespace)
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(_, name)| std::cmp::Reverse(name.len()));
        if let Some((model, name)) = candidates.into_iter().next() {
            let effort = args[name.len()..].trim();
            let Some(option) = self
                .reasoning_effort_options(model)
                .into_iter()
                .find(|option| {
                    option
                        .reasoning_effort
                        .as_str()
                        .eq_ignore_ascii_case(effort)
                })
            else {
                return Err(ModelResolveError::UnsupportedEffort {
                    model: model.display_name.clone(),
                    effort: effort.to_string(),
                });
            };
            return Ok(selection(model, option.reasoning_effort));
        }
        Err(ModelResolveError::UnknownModel(args.to_string()))
    }

    pub(crate) fn resolve_effort(&self, effort: &str) -> Result<ModelSelection, ModelResolveError> {
        let model = self
            .current_model()
            .ok_or_else(|| ModelResolveError::UnknownModel(self.current_model.clone()))?;
        let effort = effort.trim();
        let Some(option) = self
            .reasoning_effort_options(model)
            .into_iter()
            .find(|option| {
                option
                    .reasoning_effort
                    .as_str()
                    .eq_ignore_ascii_case(effort)
            })
        else {
            return Err(ModelResolveError::UnsupportedEffort {
                model: model.display_name.clone(),
                effort: effort.to_string(),
            });
        };
        Ok(selection(model, option.reasoning_effort))
    }

    fn effort_suggestions_for(
        &self,
        model: &Model,
        query: &str,
        insert_text: impl Fn(&ReasoningEffort) -> String,
    ) -> Vec<ModelSuggestion> {
        let mark_active = self.is_current(model);
        self.reasoning_effort_options(model)
            .into_iter()
            .filter(|option| fuzzy_match(option.reasoning_effort.as_str(), query).is_some())
            .map(|option| {
                let effort = option.reasoning_effort;
                ModelSuggestion {
                    display: if mark_active && self.current_effort.as_ref() == Some(&effort) {
                        format!("{effort} (active)")
                    } else {
                        effort.to_string()
                    },
                    description: option.description,
                    insert_text: insert_text(&effort),
                }
            })
            .collect()
    }

    fn effort_phase<'a>(&'a self, args_query: &'a str) -> Option<(&'a Model, &'a str)> {
        let mut matches = self
            .models
            .iter()
            .flat_map(|model| model_names(model).map(move |name| (model, name)))
            .filter(|(_, name)| {
                args_query.len() > name.len()
                    && args_query
                        .get(..name.len())
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(name))
                    && args_query[name.len()..].starts_with(char::is_whitespace)
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(_, name)| std::cmp::Reverse(name.len()));
        matches
            .first()
            .map(|(model, name)| (*model, args_query[name.len()..].trim_start()))
    }

    fn is_current(&self, model: &Model) -> bool {
        model.model == self.current_model && model.model_provider == self.current_provider
    }

    fn current_model(&self) -> Option<&Model> {
        self.models.iter().find(|model| self.is_current(model))
    }

    fn supports_reasoning_effort(&self, model: &Model) -> bool {
        !model.supported_reasoning_efforts.is_empty()
            || model.capabilities.supports_reasoning == Some(true)
            || model.default_reasoning_effort != ReasoningEffort::None
            || self
                .observed_reasoning_models
                .contains(&model_identity(model))
    }

    fn reasoning_effort_options(&self, model: &Model) -> Vec<ReasoningEffortOption> {
        if !model.supported_reasoning_efforts.is_empty() {
            return model.supported_reasoning_efforts.clone();
        }
        if !self.supports_reasoning_effort(model) {
            return vec![ReasoningEffortOption {
                reasoning_effort: ReasoningEffort::None,
                description: "No reasoning".to_string(),
            }];
        }
        LEGACY_REASONING_EFFORTS
            .iter()
            .map(|(reasoning_effort, description)| ReasoningEffortOption {
                reasoning_effort: reasoning_effort.clone(),
                description: (*description).to_string(),
            })
            .collect()
    }

    fn remember_current_reasoning_model(&mut self) {
        if self
            .current_effort
            .as_ref()
            .is_some_and(|effort| effort != &ReasoningEffort::None)
        {
            self.observed_reasoning_models
                .insert((self.current_provider.clone(), self.current_model.clone()));
        }
    }
}

fn model_match(model: &Model, query: &str) -> Option<u32> {
    model_names(model)
        .filter_map(|name| fuzzy_match(name, query).map(|(score, _)| score))
        .max()
}

fn model_name_matches(model: &Model, name: &str) -> bool {
    model_names(model).any(|candidate| candidate.eq_ignore_ascii_case(name))
}

fn model_names(model: &Model) -> impl Iterator<Item = &str> {
    [
        model.display_name.as_str(),
        model.model.as_str(),
        model.id.as_str(),
    ]
    .into_iter()
}

fn model_identity(model: &Model) -> (String, String) {
    (model.model_provider.clone(), model.model.clone())
}

fn selection(model: &Model, effort: ReasoningEffort) -> ModelSelection {
    ModelSelection {
        model: model.model.clone(),
        model_provider: model.model_provider.clone(),
        display_name: model.display_name.clone(),
        effort,
    }
}

#[cfg(test)]
#[path = "model_command_tests.rs"]
mod tests;
