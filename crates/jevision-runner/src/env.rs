//! The environment a run uses: its values for `${env.NAME}`.

use std::path::Path;

use jevision_core::{
    ENVIRONMENTS_FILE, EnvironmentValues, Environments, Flow, required_env_values,
};

use crate::report::Failure;

/// The environment chosen for a run, as read from the project.
pub(crate) struct Environment {
    pub name: String,
    pub values: EnvironmentValues,
}

/// Reads the environment `name` from the project folder; `None` when the run has no
/// environment.
pub(crate) fn load(project: &Path, name: Option<&str>) -> Result<Option<Environment>, Failure> {
    let Some(name) = name else { return Ok(None) };
    let environments = Environments::load_checked(project).map_err(|error| {
        if error.requires_newer() {
            let issue = error.issue(ENVIRONMENTS_FILE, "flow.environments.invalid");
            return Failure {
                code: issue.code,
                params: issue
                    .params
                    .into_iter()
                    .map(|param| (param.name, param.value))
                    .collect(),
                message: String::new(),
            };
        }
        Failure::new("run.environments.unreadable")
            .with("file", ENVIRONMENTS_FILE)
            .with("reason", error)
    })?;
    let values = environments.get(name).cloned().ok_or_else(|| {
        Failure::new("run.env.unknown")
            .with("environment", name)
            .with("file", ENVIRONMENTS_FILE)
    })?;
    Ok(Some(Environment {
        name: name.to_owned(),
        values,
    }))
}

/// Fails unless `environment` has every value `flow` reads, before anything runs.
pub(crate) fn check(flow: &Flow, environment: Option<&Environment>) -> Result<(), Failure> {
    for name in required_env_values(flow) {
        match environment {
            None => return Err(Failure::new("run.env.notChosen").with("name", name)),
            Some(env) if !env.values.contains_key(&name) => {
                return Err(Failure::new("run.env.missing")
                    .with("name", name)
                    .with("environment", &env.name));
            }
            Some(_) => {}
        }
    }
    Ok(())
}
