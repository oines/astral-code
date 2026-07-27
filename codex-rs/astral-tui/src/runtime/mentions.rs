use crate::AstralSession;
use crate::SurfaceState;
use crate::mention::MentionCatalog;

pub(super) async fn refresh_catalog(session: &mut AstralSession, surface: &mut SurfaceState) {
    let (skills, skills_error) = match session.list_skills().await {
        Ok(skills) => (Some(skills), None),
        Err(error) => (None, Some(format!("skills: {error}"))),
    };
    let (plugins, plugins_error) = match session.list_plugins().await {
        Ok(plugins) => (Some(plugins), None),
        Err(error) => (None, Some(format!("plugins: {error}"))),
    };
    surface.set_mention_catalog(MentionCatalog::from_inventory(
        skills.as_ref(),
        plugins.as_ref(),
    ));

    let errors = [skills_error, plugins_error]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        surface.set_notice(format!(
            "Could not load all composer mentions ({})",
            errors.join("; ")
        ));
    }
}
