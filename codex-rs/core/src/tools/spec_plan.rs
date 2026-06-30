use crate::agent::exceeds_thread_spawn_depth_limit;
use crate::agent::next_thread_spawn_depth;
use crate::session::turn_context::TurnContext;
use crate::tools::code_mode::execute_spec::create_code_mode_tool;
use crate::tools::context::ToolInvocation;
use crate::tools::handlers::ApplyPatchHandler;
use crate::tools::handlers::AstralAskUserQuestionHandler;
use crate::tools::handlers::AstralBashHandler;
use crate::tools::handlers::AstralFileToolHandler;
use crate::tools::handlers::AstralFileToolKind;
use crate::tools::handlers::AstralListBackgroundTasksHandler;
use crate::tools::handlers::AstralListMcpResourcesHandler;
use crate::tools::handlers::AstralReadMcpResourceHandler;
use crate::tools::handlers::AstralReadTaskOutputHandler;
use crate::tools::handlers::AstralRequestPermissionsHandler;
use crate::tools::handlers::AstralSendTaskInputHandler;
use crate::tools::handlers::AstralSkillHandler;
use crate::tools::handlers::AstralStopBackgroundTaskHandler;
use crate::tools::handlers::AstralTodoWriteHandler;
use crate::tools::handlers::CodeModeExecuteHandler;
use crate::tools::handlers::CodeModeWaitHandler;
use crate::tools::handlers::DynamicToolHandler;
use crate::tools::handlers::ExecCommandHandler;
use crate::tools::handlers::ExecCommandHandlerOptions;
use crate::tools::handlers::ListAvailablePluginsToInstallHandler;
use crate::tools::handlers::ListMcpResourceTemplatesHandler;
use crate::tools::handlers::ListMcpResourcesHandler;
use crate::tools::handlers::McpHandler;
use crate::tools::handlers::PlanHandler;
use crate::tools::handlers::ReadMcpResourceHandler;
use crate::tools::handlers::RequestPermissionsHandler;
use crate::tools::handlers::RequestPluginInstallHandler;
use crate::tools::handlers::RequestUserInputHandler;
use crate::tools::handlers::ShellCommandHandler;
use crate::tools::handlers::ShellCommandHandlerOptions;
use crate::tools::handlers::TestSyncHandler;
use crate::tools::handlers::ToolSearchHandler;
use crate::tools::handlers::ViewImageHandler;
use crate::tools::handlers::WriteStdinHandler;
use crate::tools::handlers::agent_jobs::ReportAgentJobResultHandler;
use crate::tools::handlers::agent_jobs::SpawnAgentsOnCsvHandler;
use crate::tools::handlers::extension_tools::ExtensionToolAdapter;
use crate::tools::handlers::multi_agents::CloseAgentHandler;
use crate::tools::handlers::multi_agents::ResumeAgentHandler;
use crate::tools::handlers::multi_agents::SendInputHandler;
use crate::tools::handlers::multi_agents::SpawnAgentHandler;
use crate::tools::handlers::multi_agents::WaitAgentHandler;
use crate::tools::handlers::multi_agents_common::DEFAULT_WAIT_TIMEOUT_MS;
use crate::tools::handlers::multi_agents_common::MAX_WAIT_TIMEOUT_MS;
use crate::tools::handlers::multi_agents_common::MIN_WAIT_TIMEOUT_MS;
use crate::tools::handlers::multi_agents_spec::SpawnAgentToolOptions;
use crate::tools::handlers::multi_agents_spec::WaitAgentTimeoutOptions;
use crate::tools::handlers::multi_agents_v2::FollowupTaskHandler as FollowupTaskHandlerV2;
use crate::tools::handlers::multi_agents_v2::InterruptAgentHandler;
use crate::tools::handlers::multi_agents_v2::ListAgentsHandler as ListAgentsHandlerV2;
use crate::tools::handlers::multi_agents_v2::SendMessageHandler as SendMessageHandlerV2;
use crate::tools::handlers::multi_agents_v2::SpawnAgentHandler as SpawnAgentHandlerV2;
use crate::tools::handlers::multi_agents_v2::WaitAgentHandler as WaitAgentHandlerV2;
use crate::tools::handlers::view_image_spec::ViewImageToolOptions;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExposure;
use crate::tools::registry::ToolRegistry;
use crate::tools::registry::override_tool_exposure;
use crate::tools::router::ToolRouter;
use crate::tools::router::ToolRouterParams;
use codex_features::Feature;
use codex_mcp::ToolInfo;
use codex_models_manager::model_info;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_tools::BASH_TOOL_NAME;
use codex_tools::DiscoverableTool;
use codex_tools::EDIT_TOOL_NAME;
use codex_tools::GLOB_TOOL_NAME;
use codex_tools::GREP_TOOL_NAME;
use codex_tools::READ_TASK_OUTPUT_TOOL_NAME;
use codex_tools::READ_TOOL_NAME;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::TODO_WRITE_TOOL_NAME;
use codex_tools::TOOL_SEARCH_TOOL_NAME;
use codex_tools::ToolCall as ExtensionToolCall;
use codex_tools::ToolEnvironmentMode;
use codex_tools::ToolExecutor;
use codex_tools::ToolName;
use codex_tools::ToolOutput;
use codex_tools::ToolSearchInfo;
use codex_tools::ToolSpec;
use codex_tools::UnifiedExecShellMode;
use codex_tools::WRITE_TOOL_NAME;
use codex_tools::astral_core_tool_by_name;
use codex_tools::can_request_original_image_detail;
use codex_tools::collect_code_mode_exec_prompt_tool_definitions;
use codex_tools::collect_request_plugin_install_entries;
use codex_tools::default_namespace_description;
use codex_tools::parse_tool_input_schema_without_compaction;
use codex_tools::request_user_input_available_modes;
use codex_tools::shell_command_backend_for_features;
use codex_tools::shell_type_for_model_and_features;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::sync::Arc;
use toml::Value as TomlValue;
use tracing::warn;

const MULTI_AGENT_V2_NAMESPACE_DESCRIPTION: &str = "Tools for spawning and managing sub-agents.";
const IMAGE_GEN_NAMESPACE: &str = "image_gen";
const IMAGEGEN_TOOL_NAME: &str = "imagegen";

type PlannedRuntime = Arc<dyn CoreToolRuntime>;

#[derive(Default)]
struct PlannedTools {
    runtimes: Vec<PlannedRuntime>,
}

impl PlannedTools {
    fn add<T>(&mut self, handler: T)
    where
        T: CoreToolRuntime + 'static,
    {
        self.runtimes.push(Arc::new(handler));
    }

    fn add_arc(&mut self, handler: PlannedRuntime) {
        self.runtimes.push(handler);
    }

    fn add_with_exposure<T>(&mut self, handler: T, exposure: ToolExposure)
    where
        T: CoreToolRuntime + 'static,
    {
        self.runtimes
            .push(override_tool_exposure(Arc::new(handler), exposure));
    }

    fn add_dispatch_only<T>(&mut self, handler: T)
    where
        T: CoreToolRuntime + 'static,
    {
        self.add_with_exposure(handler, ToolExposure::Hidden);
    }

    fn runtimes(&self) -> &[PlannedRuntime] {
        &self.runtimes
    }
}

#[derive(Clone, Copy)]
struct CoreToolPlanContext<'a> {
    turn_context: &'a TurnContext,
    mcp_tools: Option<&'a [ToolInfo]>,
    deferred_mcp_tools: Option<&'a [ToolInfo]>,
    discoverable_tools: Option<&'a [DiscoverableTool]>,
    extension_tool_executors: &'a [Arc<dyn ToolExecutor<ExtensionToolCall>>],
    dynamic_tools: &'a [DynamicToolSpec],
    default_agent_type_description: &'a str,
    wait_agent_timeouts: WaitAgentTimeoutOptions,
}

pub(crate) fn build_tool_router(
    turn_context: &TurnContext,
    params: ToolRouterParams<'_>,
) -> ToolRouter {
    let (model_visible_specs, registry) = build_tool_specs_and_registry(turn_context, params);
    ToolRouter::from_parts(registry, model_visible_specs)
}

fn build_tool_specs_and_registry(
    turn_context: &TurnContext,
    params: ToolRouterParams<'_>,
) -> (Vec<ToolSpec>, ToolRegistry) {
    let ToolRouterParams {
        mcp_tools,
        deferred_mcp_tools,
        discoverable_tools,
        extension_tool_executors,
        dynamic_tools,
    } = params;
    let default_agent_type_description =
        crate::agent::role::spawn_tool_spec::build(&std::collections::BTreeMap::new());
    let context = CoreToolPlanContext {
        turn_context,
        mcp_tools: mcp_tools.as_deref(),
        deferred_mcp_tools: deferred_mcp_tools.as_deref(),
        discoverable_tools: discoverable_tools.as_deref(),
        extension_tool_executors: &extension_tool_executors,
        dynamic_tools,
        default_agent_type_description: &default_agent_type_description,
        wait_agent_timeouts: wait_agent_timeout_options(turn_context),
    };
    let mut planned_tools = PlannedTools::default();
    add_tool_sources(&context, &mut planned_tools);
    append_tool_search_executor(&context, &mut planned_tools);
    prepend_code_mode_executors(&context, &mut planned_tools);
    build_model_visible_specs_and_registry(turn_context, planned_tools)
}

fn build_model_visible_specs_and_registry(
    turn_context: &TurnContext,
    planned_tools: PlannedTools,
) -> (Vec<ToolSpec>, ToolRegistry) {
    let PlannedTools { runtimes } = planned_tools;
    let mut specs = Vec::new();
    let mut seen_tool_names = HashSet::new();
    let registered_tool_names = runtimes
        .iter()
        .map(|runtime| runtime.tool_name())
        .collect::<HashSet<_>>();
    let mut seen_model_visible_names = HashSet::new();
    for runtime in &runtimes {
        let tool_name = runtime.tool_name();
        if !seen_tool_names.insert(tool_name.clone()) {
            continue;
        }
        let exposure = runtime.exposure();
        if exposure.is_direct() && !is_hidden_by_code_mode_only(turn_context, &tool_name, exposure)
        {
            let spec = runtime.spec();
            let spec = spec_for_model_request(turn_context, exposure, &tool_name, spec);
            let spec = astral_spec_for_model_request(&tool_name, spec, &registered_tool_names);
            push_model_visible_spec(&mut specs, &mut seen_model_visible_names, spec);
        }
    }
    append_astral_file_search_specs(
        turn_context,
        &registered_tool_names,
        &mut specs,
        &mut seen_model_visible_names,
    );
    let registry = ToolRegistry::from_tools(runtimes);
    let model_visible_specs = merge_into_namespaces(specs)
        .into_iter()
        .filter(|spec| {
            namespace_tools_enabled(turn_context) || !matches!(spec, ToolSpec::Namespace(_))
        })
        .collect();

    (model_visible_specs, registry)
}

fn push_model_visible_spec(
    specs: &mut Vec<ToolSpec>,
    seen_names: &mut HashSet<String>,
    spec: ToolSpec,
) {
    if matches!(spec, ToolSpec::Namespace(_)) {
        specs.push(spec);
        return;
    }

    if seen_names.insert(spec.name().to_string()) {
        specs.push(spec);
    }
}

fn append_astral_file_search_specs(
    turn_context: &TurnContext,
    registered_tool_names: &HashSet<ToolName>,
    specs: &mut Vec<ToolSpec>,
    seen_names: &mut HashSet<String>,
) {
    if matches!(
        turn_context.tool_mode,
        ToolMode::CodeMode | ToolMode::CodeModeOnly
    ) {
        return;
    }

    if !registered_tool_names.contains(&ToolName::plain("exec_command")) {
        return;
    }

    for tool_name in [
        READ_TOOL_NAME,
        WRITE_TOOL_NAME,
        EDIT_TOOL_NAME,
        GLOB_TOOL_NAME,
        GREP_TOOL_NAME,
    ] {
        push_model_visible_spec(specs, seen_names, astral_tool_spec(tool_name));
    }
}

fn astral_spec_for_model_request(
    tool_name: &ToolName,
    spec: ToolSpec,
    registered_tool_names: &HashSet<ToolName>,
) -> ToolSpec {
    if tool_name.namespace.is_some() {
        return spec;
    }

    let astral_tool_name = match tool_name.name.as_str() {
        "exec_command" => Some(BASH_TOOL_NAME),
        "write_stdin" => Some(READ_TASK_OUTPUT_TOOL_NAME),
        "shell_command" if registered_tool_names.contains(&ToolName::plain("exec_command")) => {
            Some(BASH_TOOL_NAME)
        }
        "update_plan" => Some(TODO_WRITE_TOOL_NAME),
        _ => None,
    };

    let Some(astral_tool_name) = astral_tool_name else {
        return spec;
    };

    astral_tool_spec_from_source(astral_tool_name, &spec)
}

fn astral_tool_spec(name: &str) -> ToolSpec {
    astral_tool_spec_from_source(
        name,
        /*source*/
        &ToolSpec::ImageGeneration {
            output_format: String::new(),
        },
    )
}

fn astral_tool_spec_from_source(name: &str, source: &ToolSpec) -> ToolSpec {
    let tool = astral_core_tool_by_name(name).unwrap_or_else(|| {
        panic!("astral core tool `{name}` should have a schema");
    });
    let parameters = parse_tool_input_schema_without_compaction(&tool.input_schema)
        .unwrap_or_else(|err| panic!("astral core tool `{name}` schema should parse: {err}"));
    let description = source_description(source).map_or(tool.description.clone(), |source| {
        if source.trim().is_empty() || source == tool.description {
            tool.description.clone()
        } else {
            format!("{}\n\n{}", tool.description, source)
        }
    });
    ToolSpec::Function(codex_tools::ResponsesApiTool {
        name: tool.name,
        description,
        strict: false,
        defer_loading: None,
        parameters,
        output_schema: None,
    })
}

fn source_description(source: &ToolSpec) -> Option<&str> {
    match source {
        ToolSpec::Function(tool) => Some(tool.description.as_str()),
        ToolSpec::ToolSearch { description, .. } => Some(description.as_str()),
        ToolSpec::Namespace(_)
        | ToolSpec::ImageGeneration { .. }
        | ToolSpec::WebSearch { .. }
        | ToolSpec::Freeform(_) => None,
    }
}

fn spec_for_model_request(
    turn_context: &TurnContext,
    exposure: ToolExposure,
    tool_name: &ToolName,
    spec: ToolSpec,
) -> ToolSpec {
    if matches!(
        turn_context.tool_mode,
        ToolMode::CodeMode | ToolMode::CodeModeOnly
    ) && exposure != ToolExposure::DirectModelOnly
        && !is_excluded_from_code_mode(turn_context, tool_name)
        && codex_code_mode::is_code_mode_nested_tool(spec.name())
    {
        codex_tools::augment_tool_spec_for_code_mode(spec)
    } else {
        spec
    }
}

pub(crate) fn search_tool_enabled(turn_context: &TurnContext) -> bool {
    turn_context.model_info.supports_search_tool
}

pub(crate) fn tool_suggest_enabled(turn_context: &TurnContext) -> bool {
    let features = turn_context.features.get();
    features.enabled(Feature::ToolSuggest)
        && features.enabled(Feature::Apps)
        && features.enabled(Feature::Plugins)
}

fn namespace_tools_enabled(turn_context: &TurnContext) -> bool {
    turn_context.provider.capabilities().namespace_tools
}

fn multi_agent_v2_enabled(turn_context: &TurnContext) -> bool {
    turn_context.multi_agent_version == MultiAgentVersion::V2
}

fn collab_tools_enabled(turn_context: &TurnContext) -> bool {
    match turn_context.multi_agent_version {
        MultiAgentVersion::Disabled => false,
        MultiAgentVersion::V1 => !exceeds_thread_spawn_depth_limit(
            next_thread_spawn_depth(&turn_context.session_source),
            turn_context.config.agent_max_depth,
        ),
        MultiAgentVersion::V2 => true,
    }
}

fn agent_jobs_tools_enabled(turn_context: &TurnContext) -> bool {
    turn_context.features.get().enabled(Feature::SpawnCsv) && collab_tools_enabled(turn_context)
}

fn agent_jobs_worker_tools_enabled(turn_context: &TurnContext) -> bool {
    agent_jobs_tools_enabled(turn_context)
        && matches!(
            &turn_context.session_source,
            SessionSource::SubAgent(SubAgentSource::Other(label))
                if label.starts_with("agent_job:")
        )
}

fn standalone_image_generation_model_visible(_turn_context: &TurnContext) -> bool {
    false
}

fn wait_agent_timeout_options(turn_context: &TurnContext) -> WaitAgentTimeoutOptions {
    if multi_agent_v2_enabled(turn_context) {
        return WaitAgentTimeoutOptions {
            default_timeout_ms: turn_context.config.multi_agent_v2.default_wait_timeout_ms,
            min_timeout_ms: turn_context.config.multi_agent_v2.min_wait_timeout_ms,
            max_timeout_ms: turn_context.config.multi_agent_v2.max_wait_timeout_ms,
        };
    }

    WaitAgentTimeoutOptions {
        default_timeout_ms: DEFAULT_WAIT_TIMEOUT_MS,
        min_timeout_ms: MIN_WAIT_TIMEOUT_MS,
        max_timeout_ms: MAX_WAIT_TIMEOUT_MS,
    }
}

fn max_concurrent_threads_per_session(turn_context: &TurnContext) -> Option<usize> {
    multi_agent_v2_enabled(turn_context).then_some(
        turn_context
            .config
            .multi_agent_v2
            .max_concurrent_threads_per_session,
    )
}

fn agent_type_description(
    turn_context: &TurnContext,
    default_agent_type_description: &str,
) -> String {
    let agent_type_description =
        crate::agent::role::spawn_tool_spec::build(&turn_context.config.agent_roles);
    if agent_type_description.is_empty() {
        default_agent_type_description.to_string()
    } else {
        agent_type_description
    }
}

fn is_hidden_by_code_mode_only(
    turn_context: &TurnContext,
    tool_name: &ToolName,
    exposure: ToolExposure,
) -> bool {
    turn_context.tool_mode == ToolMode::CodeModeOnly
        && exposure != ToolExposure::DirectModelOnly
        && codex_code_mode::is_code_mode_nested_tool(&codex_tools::code_mode_name_for_tool_name(
            tool_name,
        ))
}

fn is_excluded_from_code_mode(turn_context: &TurnContext, tool_name: &ToolName) -> bool {
    tool_name.namespace.as_ref().is_some_and(|namespace| {
        turn_context
            .config
            .code_mode
            .excluded_tool_namespaces
            .contains(namespace)
    })
}

fn build_code_mode_executors(
    turn_context: &TurnContext,
    executors: &[Arc<dyn CoreToolRuntime>],
) -> Vec<Arc<dyn CoreToolRuntime>> {
    if !matches!(
        turn_context.tool_mode,
        ToolMode::CodeMode | ToolMode::CodeModeOnly
    ) {
        return vec![];
    }

    let mut code_mode_nested_tool_specs = Vec::new();
    let mut exec_prompt_tool_specs = Vec::new();
    let mut deferred_tools_available = false;
    let deferred_tools_guidance_enabled = search_tool_enabled(turn_context);
    for executor in executors {
        let exposure = executor.exposure();
        let tool_name = executor.tool_name();
        if exposure == ToolExposure::DirectModelOnly {
            continue;
        }

        if exposure == ToolExposure::Hidden {
            if tool_name.namespace.is_none()
                && matches!(tool_name.name.as_str(), "exec_command" | "shell_command")
            {
                code_mode_nested_tool_specs.push(executor.spec());
            }
            continue;
        }

        if is_excluded_from_code_mode(turn_context, &tool_name) {
            continue;
        }

        let spec = executor.spec();

        if exposure == ToolExposure::Deferred {
            // Only show deferred-tool guidance when supported and an included spec is usable by code mode.
            deferred_tools_available |= deferred_tools_guidance_enabled
                && !collect_code_mode_exec_prompt_tool_definitions(std::iter::once(&spec))
                    .is_empty();
        } else {
            exec_prompt_tool_specs.push(spec.clone());
        }
        code_mode_nested_tool_specs.push(spec);
    }

    let namespace_descriptions = code_mode_namespace_descriptions(&exec_prompt_tool_specs);
    let mut enabled_tools =
        collect_code_mode_exec_prompt_tool_definitions(exec_prompt_tool_specs.iter());
    enabled_tools
        .sort_by(|left, right| compare_code_mode_tools(left, right, &namespace_descriptions));

    vec![
        Arc::new(CodeModeExecuteHandler::new(
            create_code_mode_tool(
                &enabled_tools,
                &namespace_descriptions,
                turn_context.tool_mode == ToolMode::CodeModeOnly,
                deferred_tools_available,
            ),
            code_mode_nested_tool_specs,
        )),
        Arc::new(CodeModeWaitHandler),
    ]
}

fn merge_into_namespaces(specs: Vec<ToolSpec>) -> Vec<ToolSpec> {
    let mut merged_specs = Vec::with_capacity(specs.len());
    let mut namespace_indices = BTreeMap::<String, usize>::new();
    for spec in specs {
        match spec {
            ToolSpec::Namespace(mut namespace) => {
                if let Some(index) = namespace_indices.get(&namespace.name).copied() {
                    let ToolSpec::Namespace(existing_namespace) = &mut merged_specs[index] else {
                        unreachable!("namespace index must point to a namespace spec");
                    };
                    if existing_namespace.description.trim().is_empty()
                        && !namespace.description.trim().is_empty()
                    {
                        existing_namespace.description = namespace.description;
                    }
                    existing_namespace.tools.append(&mut namespace.tools);
                    continue;
                }

                namespace_indices.insert(namespace.name.clone(), merged_specs.len());
                merged_specs.push(ToolSpec::Namespace(namespace));
            }
            spec => merged_specs.push(spec),
        }
    }

    for spec in &mut merged_specs {
        let ToolSpec::Namespace(namespace) = spec else {
            continue;
        };

        namespace.tools.sort_by(|left, right| match (left, right) {
            (
                ResponsesApiNamespaceTool::Function(left),
                ResponsesApiNamespaceTool::Function(right),
            ) => left.name.cmp(&right.name),
        });

        if namespace.description.trim().is_empty() {
            namespace.description = default_namespace_description(&namespace.name);
        }
    }

    merged_specs
}

fn code_mode_namespace_descriptions(
    specs: &[ToolSpec],
) -> BTreeMap<String, codex_code_mode::ToolNamespaceDescription> {
    let mut namespace_descriptions = BTreeMap::new();
    for spec in specs {
        let ToolSpec::Namespace(namespace) = spec else {
            continue;
        };

        let entry = namespace_descriptions
            .entry(namespace.name.clone())
            .or_insert_with(|| codex_code_mode::ToolNamespaceDescription {
                name: namespace.name.clone(),
                description: namespace.description.clone(),
            });
        if entry.description.trim().is_empty() && !namespace.description.trim().is_empty() {
            entry.description = namespace.description.clone();
        }
    }
    namespace_descriptions
}

fn add_tool_sources(context: &CoreToolPlanContext<'_>, planned_tools: &mut PlannedTools) {
    add_shell_tools(context, planned_tools);
    add_astral_file_tools(context, planned_tools);
    add_mcp_resource_tools(context, planned_tools);
    add_core_utility_tools(context, planned_tools);
    add_collaboration_tools(context, planned_tools);
    add_mcp_runtime_tools(context, planned_tools);
    add_extension_tools(context, planned_tools);
    add_dynamic_tools(context, planned_tools);
}

fn add_astral_file_tools(context: &CoreToolPlanContext<'_>, planned_tools: &mut PlannedTools) {
    if matches!(
        context.turn_context.tool_mode,
        ToolMode::CodeMode | ToolMode::CodeModeOnly
    ) {
        return;
    }
    if !context
        .turn_context
        .tool_environment_mode()
        .has_environment()
    {
        return;
    }

    for kind in [
        AstralFileToolKind::Read,
        AstralFileToolKind::Write,
        AstralFileToolKind::Edit,
        AstralFileToolKind::Glob,
        AstralFileToolKind::Grep,
    ] {
        planned_tools.add(AstralFileToolHandler::new(kind));
    }
}

fn provider_neutral_web_tools_enabled(turn_context: &TurnContext) -> bool {
    turn_context.config.web_search_mode.value() == WebSearchMode::Live
}

fn add_shell_tools(context: &CoreToolPlanContext<'_>, planned_tools: &mut PlannedTools) {
    let turn_context = context.turn_context;
    let features = turn_context.features.get();
    let environment_mode = turn_context.tool_environment_mode();
    if !environment_mode.has_environment() {
        return;
    }

    let allow_login_shell = turn_context.config.permissions.allow_login_shell;
    let exec_permission_approvals_enabled = features.enabled(Feature::ExecPermissionApprovals);
    let include_environment_id = matches!(environment_mode, ToolEnvironmentMode::Multiple);
    let shell_command_options = ShellCommandHandlerOptions {
        backend_config: shell_command_backend_for_features(features),
        allow_login_shell,
        exec_permission_approvals_enabled,
    };

    match shell_type_for_model_and_features(&turn_context.model_info, features) {
        ConfigShellToolType::UnifiedExec => {
            let exec_options = ExecCommandHandlerOptions {
                allow_login_shell,
                exec_permission_approvals_enabled,
                include_environment_id,
                include_shell_parameter: unified_exec_should_include_shell_parameter(turn_context),
            };
            planned_tools.add(AstralBashHandler::new(exec_options));
            planned_tools.add(AstralReadTaskOutputHandler::new());
            planned_tools.add(AstralSendTaskInputHandler::new());
            planned_tools.add(AstralListBackgroundTasksHandler);
            planned_tools.add(AstralStopBackgroundTaskHandler);
            planned_tools.add_dispatch_only(ExecCommandHandler::new(exec_options));
            planned_tools.add_dispatch_only(WriteStdinHandler);

            // Keep the legacy shell tool registered while unified exec is
            // model-visible.
            planned_tools.add_dispatch_only(ShellCommandHandler::new(shell_command_options));
        }
        ConfigShellToolType::Disabled => {}
        ConfigShellToolType::Default
        | ConfigShellToolType::Local
        | ConfigShellToolType::ShellCommand => {
            planned_tools.add(AstralBashHandler::new_shell_command(shell_command_options));
            planned_tools.add_dispatch_only(ShellCommandHandler::new(shell_command_options));
        }
    }
}

fn unified_exec_should_include_shell_parameter(turn_context: &TurnContext) -> bool {
    !matches!(
        &turn_context.unified_exec_shell_mode,
        UnifiedExecShellMode::ZshFork(_)
    ) || turn_context
        .environments
        .turn_environments
        .iter()
        .any(|environment| environment.environment.is_remote())
}

fn add_mcp_resource_tools(context: &CoreToolPlanContext<'_>, planned_tools: &mut PlannedTools) {
    if context.mcp_tools.is_some() {
        planned_tools.add(AstralListMcpResourcesHandler::new());
        planned_tools.add_dispatch_only(ListMcpResourcesHandler);
        planned_tools.add(ListMcpResourceTemplatesHandler);
        planned_tools.add(AstralReadMcpResourceHandler::new());
        planned_tools.add_dispatch_only(ReadMcpResourceHandler);
    }
}

fn add_core_utility_tools(context: &CoreToolPlanContext<'_>, planned_tools: &mut PlannedTools) {
    let turn_context = context.turn_context;
    let features = turn_context.features.get();
    let environment_mode = turn_context.tool_environment_mode();

    planned_tools.add(AstralTodoWriteHandler::new());
    planned_tools.add_dispatch_only(PlanHandler);

    if turn_context
        .turn_skills
        .outcome
        .skills_with_enabled()
        .any(|(_, enabled)| enabled)
    {
        planned_tools.add(AstralSkillHandler);
    }

    if turn_context.config.experimental_request_user_input_enabled {
        let available_modes = request_user_input_available_modes(features);
        planned_tools.add(AstralAskUserQuestionHandler::new(available_modes.clone()));
        planned_tools.add_dispatch_only(RequestUserInputHandler { available_modes });
    }

    if features.enabled(Feature::RequestPermissionsTool) {
        planned_tools.add(AstralRequestPermissionsHandler::new());
        planned_tools.add_dispatch_only(RequestPermissionsHandler);
    }

    if tool_suggest_enabled(turn_context)
        && let Some(discoverable_tools) =
            context.discoverable_tools.filter(|tools| !tools.is_empty())
    {
        planned_tools.add(ListAvailablePluginsToInstallHandler::new(
            collect_request_plugin_install_entries(discoverable_tools),
        ));
        planned_tools.add(RequestPluginInstallHandler::new(
            discoverable_tools.to_vec(),
        ));
    }

    if environment_mode.has_environment() && turn_context.model_info.apply_patch_tool_type.is_some()
    {
        let include_environment_id = matches!(environment_mode, ToolEnvironmentMode::Multiple);
        planned_tools.add_dispatch_only(ApplyPatchHandler::new(include_environment_id));
    }

    if turn_context
        .model_info
        .experimental_supported_tools
        .iter()
        .any(|tool| tool == "test_sync_tool")
    {
        planned_tools.add(TestSyncHandler);
    }

    if environment_mode.has_environment() {
        let include_environment_id = matches!(environment_mode, ToolEnvironmentMode::Multiple);
        planned_tools.add_dispatch_only(ViewImageHandler::new(ViewImageToolOptions {
            can_request_original_image_detail: can_request_original_image_detail(
                &turn_context.model_info,
            ),
            include_environment_id,
        }));
    }
}

fn add_collaboration_tools(context: &CoreToolPlanContext<'_>, planned_tools: &mut PlannedTools) {
    let turn_context = context.turn_context;
    if collab_tools_enabled(turn_context) {
        let available_models = spawn_agent_available_models(turn_context);
        if multi_agent_v2_enabled(turn_context) {
            let exposure = if turn_context.config.multi_agent_v2.non_code_mode_only {
                ToolExposure::DirectModelOnly
            } else {
                ToolExposure::Direct
            };
            let tool_namespace = namespace_tools_enabled(turn_context)
                .then_some(turn_context.config.multi_agent_v2.tool_namespace.as_deref())
                .flatten();
            let agent_type_description =
                agent_type_description(turn_context, context.default_agent_type_description);
            planned_tools.add_arc(override_tool_exposure(
                multi_agent_v2_handler(
                    SpawnAgentHandlerV2::new(SpawnAgentToolOptions {
                        available_models: available_models.clone(),
                        agent_type_description,
                        hide_agent_type_model_reasoning: turn_context
                            .config
                            .multi_agent_v2
                            .hide_spawn_agent_metadata,
                        include_usage_hint: turn_context.config.multi_agent_v2.usage_hint_enabled,
                        usage_hint_text: turn_context.config.multi_agent_v2.usage_hint_text.clone(),
                        max_concurrent_threads_per_session: max_concurrent_threads_per_session(
                            turn_context,
                        ),
                    }),
                    tool_namespace,
                ),
                exposure,
            ));
            planned_tools.add_arc(override_tool_exposure(
                multi_agent_v2_handler(SendMessageHandlerV2, tool_namespace),
                exposure,
            ));
            planned_tools.add_arc(override_tool_exposure(
                multi_agent_v2_handler(FollowupTaskHandlerV2, tool_namespace),
                exposure,
            ));
            planned_tools.add_arc(override_tool_exposure(
                multi_agent_v2_handler(
                    WaitAgentHandlerV2::new(context.wait_agent_timeouts),
                    tool_namespace,
                ),
                exposure,
            ));
            planned_tools.add_arc(override_tool_exposure(
                multi_agent_v2_handler(InterruptAgentHandler, tool_namespace),
                exposure,
            ));
            planned_tools.add_arc(override_tool_exposure(
                multi_agent_v2_handler(ListAgentsHandlerV2, tool_namespace),
                exposure,
            ));
        } else {
            let agent_type_description =
                agent_type_description(turn_context, context.default_agent_type_description);
            let exposure =
                if search_tool_enabled(turn_context) && namespace_tools_enabled(turn_context) {
                    ToolExposure::Deferred
                } else {
                    ToolExposure::Direct
                };
            planned_tools.add_with_exposure(
                SpawnAgentHandler::new(SpawnAgentToolOptions {
                    available_models,
                    agent_type_description,
                    hide_agent_type_model_reasoning: false,
                    include_usage_hint: turn_context.config.multi_agent_v2.usage_hint_enabled,
                    usage_hint_text: turn_context.config.multi_agent_v2.usage_hint_text.clone(),
                    max_concurrent_threads_per_session: max_concurrent_threads_per_session(
                        turn_context,
                    ),
                }),
                exposure,
            );
            planned_tools.add_with_exposure(SendInputHandler, exposure);
            planned_tools.add_with_exposure(ResumeAgentHandler, exposure);
            planned_tools
                .add_with_exposure(WaitAgentHandler::new(context.wait_agent_timeouts), exposure);
            planned_tools.add_with_exposure(CloseAgentHandler, exposure);
        }
    }

    if agent_jobs_tools_enabled(turn_context) {
        planned_tools.add(SpawnAgentsOnCsvHandler);
        if agent_jobs_worker_tools_enabled(turn_context) {
            planned_tools.add(ReportAgentJobResultHandler);
        }
    }
}

fn spawn_agent_available_models(turn_context: &TurnContext) -> Vec<ModelPreset> {
    let current_provider_id = turn_context.config.model_provider_id.as_str();
    let mut seen = BTreeSet::new();
    let mut available_models = Vec::new();

    for mut model in turn_context.available_models.clone() {
        let provider_id = model
            .model_provider
            .clone()
            .unwrap_or_else(|| current_provider_id.to_string());
        model.model_provider = Some(provider_id.clone());
        model.model_provider_name = None;
        if seen.insert((provider_id, model.model.clone())) {
            available_models.push(model);
        }
    }

    for (provider_id, model) in spawn_agent_configured_model_specs(turn_context) {
        if !turn_context
            .config
            .model_providers
            .contains_key(provider_id.as_str())
        {
            continue;
        }
        if seen.insert((provider_id.clone(), model.clone())) {
            available_models.push(spawn_agent_configured_model_preset(
                turn_context,
                provider_id.as_str(),
                model.as_str(),
            ));
        }
    }

    available_models
}

fn spawn_agent_configured_model_specs(turn_context: &TurnContext) -> Vec<(String, String)> {
    let config = &turn_context.config;
    let mut specs = BTreeSet::new();
    if let Some(model_name) = config.model.as_ref() {
        specs.insert((config.model_provider_id.clone(), model_name.clone()));
    }

    if let Some(TomlValue::Table(model_capabilities)) = config
        .config_layer_stack
        .effective_config()
        .get("model_capabilities")
    {
        for model_key in model_capabilities.keys() {
            if let Some(spec) = spawn_agent_configured_model_spec_from_key(
                model_key,
                config.model_provider_id.as_str(),
            ) {
                specs.insert(spec);
            }
        }
    }

    specs.into_iter().collect()
}

fn spawn_agent_configured_model_spec_from_key(
    model_key: &str,
    default_provider_id: &str,
) -> Option<(String, String)> {
    let (provider_id, model_name) = model_key.split_once('/').map_or(
        (default_provider_id, model_key),
        |(provider_id, model_name)| (provider_id, model_name),
    );
    let provider_id = provider_id.trim();
    let model_name = model_name.trim();
    if provider_id.is_empty() || model_name.is_empty() {
        return None;
    }
    Some((provider_id.to_string(), model_name.to_string()))
}

fn spawn_agent_configured_model_preset(
    turn_context: &TurnContext,
    provider_id: &str,
    model: &str,
) -> ModelPreset {
    let mut model_info = turn_context
        .config
        .model_catalog
        .as_ref()
        .and_then(|catalog| {
            let provider_model = format!("{provider_id}/{model}");
            catalog
                .models
                .iter()
                .find(|candidate| candidate.slug == provider_model || candidate.slug == model)
                .cloned()
        })
        .unwrap_or_else(|| model_info::model_info_from_slug(model));
    model_info.slug = model.to_string();

    let mut models_manager_config = turn_context.config.to_models_manager_config();
    models_manager_config.model_provider_id = Some(provider_id.to_string());
    let model_info = model_info::with_config_overrides(model_info, &models_manager_config);
    let mut preset = ModelPreset::from(model_info);
    preset.id = format!("{provider_id}/{model}");
    preset.model_provider = Some(provider_id.to_string());
    preset.model_provider_name = None;
    preset.model = model.to_string();
    preset.display_name = model.to_string();
    preset.show_in_picker = true;
    preset.supported_in_api = true;
    preset
}

fn add_mcp_runtime_tools(context: &CoreToolPlanContext<'_>, planned_tools: &mut PlannedTools) {
    if let Some(mcp_tools) = context.mcp_tools {
        for tool in mcp_tools {
            match McpHandler::new(tool.clone()) {
                Ok(handler) => planned_tools.add(handler),
                Err(err) => warn!(
                    "Skipping MCP tool `{}`: failed to build tool spec: {err}",
                    tool.canonical_tool_name()
                ),
            }
        }
    }

    if let Some(deferred_mcp_tools) = context.deferred_mcp_tools {
        for tool in deferred_mcp_tools {
            match McpHandler::new(tool.clone()) {
                Ok(handler) => planned_tools.add_with_exposure(handler, ToolExposure::Deferred),
                Err(err) => warn!(
                    "Skipping deferred MCP tool `{}`: failed to build tool spec: {err}",
                    tool.canonical_tool_name()
                ),
            }
        }
    }
}

fn add_dynamic_tools(context: &CoreToolPlanContext<'_>, planned_tools: &mut PlannedTools) {
    for tool in context.dynamic_tools {
        let Some(handler) = DynamicToolHandler::new(tool) else {
            tracing::error!(
                "Failed to convert dynamic tool {:?} to OpenAI tool",
                tool.name
            );
            continue;
        };

        planned_tools.add(handler);
    }
}

fn add_extension_tools(context: &CoreToolPlanContext<'_>, planned_tools: &mut PlannedTools) {
    // Extension ToolContributor implementations are resolved into executors
    // before planning. Core only adapts those executors into its runtime set.
    append_extension_tool_executors(
        context.turn_context,
        context.extension_tool_executors,
        planned_tools,
    );
}

fn append_tool_search_executor(
    context: &CoreToolPlanContext<'_>,
    planned_tools: &mut PlannedTools,
) {
    let turn_context = context.turn_context;
    if !(search_tool_enabled(turn_context) && namespace_tools_enabled(turn_context)) {
        return;
    }

    let search_infos = planned_tools
        .runtimes()
        .iter()
        .filter(|executor| executor.exposure() == ToolExposure::Deferred)
        .filter_map(|executor| executor.search_info())
        .collect::<Vec<_>>();
    if search_infos.is_empty() {
        return;
    }

    planned_tools.add(ToolSearchHandler::new(search_infos));
}

fn prepend_code_mode_executors(
    context: &CoreToolPlanContext<'_>,
    planned_tools: &mut PlannedTools,
) {
    let turn_context = context.turn_context;
    let code_mode_executors = build_code_mode_executors(turn_context, planned_tools.runtimes());
    planned_tools.runtimes.splice(0..0, code_mode_executors);
}

fn append_extension_tool_executors(
    turn_context: &TurnContext,
    executors: &[Arc<dyn ToolExecutor<ExtensionToolCall>>],
    planned_tools: &mut PlannedTools,
) {
    if executors.is_empty() {
        return;
    }

    let mut reserved_tool_names = planned_tools
        .runtimes()
        .iter()
        .map(|executor| executor.tool_name())
        .collect::<HashSet<_>>();
    if matches!(
        turn_context.tool_mode,
        ToolMode::CodeMode | ToolMode::CodeModeOnly
    ) {
        reserved_tool_names.insert(ToolName::plain(codex_code_mode::PUBLIC_TOOL_NAME));
        reserved_tool_names.insert(ToolName::plain(codex_code_mode::WAIT_TOOL_NAME));
    }
    if search_tool_enabled(turn_context)
        && namespace_tools_enabled(turn_context)
        && planned_tools
            .runtimes()
            .iter()
            .any(|executor| executor.exposure() == ToolExposure::Deferred)
    {
        reserved_tool_names.insert(ToolName::plain(TOOL_SEARCH_TOOL_NAME));
    }

    let provider_neutral_web_tools_enabled = provider_neutral_web_tools_enabled(turn_context);

    for executor in executors.iter().cloned() {
        let tool_name = executor.tool_name();
        if tool_name == ToolName::namespaced("web", "run") {
            continue;
        }
        if is_provider_neutral_web_tool(&tool_name) && !provider_neutral_web_tools_enabled {
            continue;
        }
        if tool_name == ToolName::namespaced(IMAGE_GEN_NAMESPACE, IMAGEGEN_TOOL_NAME)
            && !standalone_image_generation_model_visible(turn_context)
        {
            continue;
        }
        if !reserved_tool_names.insert(tool_name.clone()) {
            warn!("Skipping extension tool `{tool_name}`: tool already registered");
            continue;
        }
        planned_tools.add(ExtensionToolAdapter::new(executor));
    }
}

fn is_provider_neutral_web_tool(tool_name: &ToolName) -> bool {
    matches!(
        (tool_name.namespace.as_deref(), tool_name.name.as_str()),
        (Some("web"), "search" | "fetch")
    )
}

fn multi_agent_v2_handler(
    handler: impl CoreToolRuntime + 'static,
    namespace: Option<&str>,
) -> Arc<dyn CoreToolRuntime> {
    match namespace {
        Some(namespace) => Arc::new(MultiAgentV2NamespaceOverride {
            handler: Arc::new(handler),
            namespace: namespace.to_string(),
        }),
        None => Arc::new(handler),
    }
}

struct MultiAgentV2NamespaceOverride {
    handler: Arc<dyn CoreToolRuntime>,
    namespace: String,
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for MultiAgentV2NamespaceOverride {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(self.namespace.clone(), self.handler.tool_name().name)
    }

    fn spec(&self) -> ToolSpec {
        match self.handler.spec() {
            ToolSpec::Function(tool) => ToolSpec::Namespace(ResponsesApiNamespace {
                name: self.namespace.clone(),
                description: MULTI_AGENT_V2_NAMESPACE_DESCRIPTION.to_string(),
                tools: vec![ResponsesApiNamespaceTool::Function(tool)],
            }),
            spec => spec,
        }
    }

    fn exposure(&self) -> ToolExposure {
        self.handler.exposure()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.handler.supports_parallel_tool_calls()
    }

    fn search_info(&self) -> Option<ToolSearchInfo> {
        self.handler.search_info()
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, codex_tools::FunctionCallError> {
        self.handler.handle(invocation).await
    }
}

impl CoreToolRuntime for MultiAgentV2NamespaceOverride {
    fn matches_kind(&self, payload: &crate::tools::context::ToolPayload) -> bool {
        self.handler.matches_kind(payload)
    }

    fn create_diff_consumer(
        &self,
    ) -> Option<Box<dyn crate::tools::registry::ToolArgumentDiffConsumer>> {
        self.handler.create_diff_consumer()
    }
}

fn compare_code_mode_tools(
    left: &codex_code_mode::ToolDefinition,
    right: &codex_code_mode::ToolDefinition,
    namespace_descriptions: &BTreeMap<String, codex_code_mode::ToolNamespaceDescription>,
) -> std::cmp::Ordering {
    let left_namespace = code_mode_namespace_name(left, namespace_descriptions);
    let right_namespace = code_mode_namespace_name(right, namespace_descriptions);

    left_namespace
        .cmp(&right_namespace)
        .then_with(|| left.tool_name.name.cmp(&right.tool_name.name))
        .then_with(|| left.name.cmp(&right.name))
}

fn code_mode_namespace_name<'a>(
    tool: &codex_code_mode::ToolDefinition,
    namespace_descriptions: &'a BTreeMap<String, codex_code_mode::ToolNamespaceDescription>,
) -> Option<&'a str> {
    tool.tool_name
        .namespace
        .as_ref()
        .and_then(|namespace| namespace_descriptions.get(namespace))
        .map(|namespace_description| namespace_description.name.as_str())
}

#[cfg(test)]
#[path = "spec_plan_tests.rs"]
mod tests;
