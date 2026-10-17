//! `pinned_references` — require immutable references in CI and container inputs.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use alint_core::{
    AnalysisFailureKind, Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope,
    SourceRange, Violation, eval_per_file,
};
use saphyr_parser::{Event, Parser, ScalarStyle, ScanError, Span, StrInput, Tag};
use schemars::JsonSchema;
use serde::Deserialize;

const MAX_YAML_NESTING: usize = 128;
const MAX_GITLAB_EXTENDS_DEPTH: usize = 11;
const MAX_PINNED_REFERENCES_BYTES: usize = alint_core::MAX_STRUCTURED_BYTES;
const LEGACY_GITHUB_STEPS_PATH: &str = "$.jobs.*.steps[*].uses";
const LEGACY_GITHUB_PIN_REGEX: &str =
    r"^(\./.*|docker://[^@]+@sha256:[a-f0-9]{64}|[a-zA-Z0-9._/-]+@[a-f0-9]{40})$";

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputFormat {
    /// Infer the consumer from the repository-relative filename.
    #[default]
    Auto,
    /// GitHub Actions workflow or action-manifest semantics.
    GithubActions,
    /// pre-commit configuration semantics.
    PreCommit,
    /// `CircleCI` configuration semantics.
    Circleci,
    /// GitLab CI configuration semantics.
    GitlabCi,
    /// Google Cloud Build configuration semantics.
    CloudBuild,
    /// Dockerfile instruction semantics.
    Dockerfile,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// Parser and semantic selector set. `auto` infers it from the path.
    #[serde(default)]
    #[schemars(extend("default" = "auto"))]
    format: InputFormat,
}

#[derive(Debug)]
pub struct PinnedReferencesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    format: InputFormat,
}

impl Rule for PinnedReferencesRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for PinnedReferencesRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        if bytes.len() > MAX_PINNED_REFERENCES_BYTES {
            return Ok(vec![Self::analysis_failure(
                path,
                AnalysisFailureKind::ResourceParse,
                format!(
                    "pinned_references input exceeds the maximum supported size \
                     ({MAX_PINNED_REFERENCES_BYTES} bytes)"
                ),
            )]);
        }
        let format = match self.format {
            InputFormat::Auto => match detect_format(path) {
                Some(format) => format,
                None => {
                    return Ok(vec![Self::analysis_failure(
                        path,
                        AnalysisFailureKind::Evaluation,
                        "could not infer a pinned_references format from the path; set `format`",
                    )]);
                }
            },
            explicit => explicit,
        };
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text.strip_prefix('\u{feff}').unwrap_or(text),
            Err(error) => {
                return Ok(vec![Self::analysis_failure(
                    path,
                    AnalysisFailureKind::ResourceParse,
                    format!("pinned_references input is not UTF-8: {error}"),
                )]);
            }
        };
        if matches!(format, InputFormat::Dockerfile) {
            return Ok(self.evaluate_dockerfile(path, text));
        }
        Ok(self.evaluate_yaml(path, text, format))
    }
}

impl PinnedReferencesRule {
    fn evaluate_yaml(&self, path: &Path, text: &str, format: InputFormat) -> Vec<Violation> {
        let document = match YamlDocument::parse(text) {
            Ok(Some(document)) => document,
            Ok(None) => return Vec::new(),
            Err(error) => {
                return vec![Self::analysis_failure(
                    path,
                    AnalysisFailureKind::ResourceParse,
                    format!("could not parse pinned_references YAML: {error}"),
                )];
            }
        };
        let mut collector = Collector::new(self, path, &document);
        let collection = match format {
            InputFormat::GithubActions => {
                collect_github_actions(&mut collector);
                Ok(())
            }
            InputFormat::PreCommit => {
                collect_pre_commit(&mut collector);
                Ok(())
            }
            InputFormat::Circleci => {
                collect_circleci(&mut collector);
                Ok(())
            }
            InputFormat::GitlabCi => collect_gitlab_ci(&mut collector),
            InputFormat::CloudBuild => {
                collect_cloud_build(&mut collector);
                Ok(())
            }
            InputFormat::Auto | InputFormat::Dockerfile => unreachable!("format resolved above"),
        };
        if let Err(error) = collection {
            return vec![Self::analysis_failure(
                path,
                AnalysisFailureKind::ResourceParse,
                format!("could not resolve pinned_references GitLab configuration: {error}"),
            )];
        }
        collector
            .findings
            .sort_by_key(|finding| (finding.line, finding.column));
        collector.findings
    }

    fn evaluate_dockerfile(&self, path: &Path, text: &str) -> Vec<Violation> {
        let instructions = match dockerfile_instructions(text) {
            Ok(instructions) => instructions,
            Err(error) => {
                return vec![Self::analysis_failure(
                    path,
                    AnalysisFailureKind::ResourceParse,
                    format!("could not parse Dockerfile: {error}"),
                )];
            }
        };
        let path = Arc::<Path>::from(path);
        let mut findings = Vec::new();
        let mut stages = HashSet::new();
        for (ordinal, instruction) in instructions.iter().enumerate() {
            if let Err(error) = collect_docker_instruction(
                self,
                &path,
                instruction,
                ordinal,
                &mut stages,
                &mut findings,
            ) {
                return vec![Self::analysis_failure(
                    &path,
                    AnalysisFailureKind::ResourceParse,
                    format!(
                        "could not parse Dockerfile instruction at line {}: {error}",
                        instruction.span.start_line
                    ),
                )];
            }
        }
        findings
    }

    fn finding(
        &self,
        path: Arc<Path>,
        span: FindingSpan,
        selector: &str,
        value: &str,
        default_message: &'static str,
    ) -> Violation {
        let baseline_key = if legacy_github_step_selector(selector) {
            crate::structured_path::matches_baseline_key(
                LEGACY_GITHUB_STEPS_PATH,
                LEGACY_GITHUB_PIN_REGEX,
                &serde_json::Value::String(value.to_owned()),
            )
        } else {
            format!("{selector}\0{value}")
        };
        let mut finding = Violation::new(
            self.message
                .clone()
                .unwrap_or_else(|| default_message.to_owned()),
        )
        .with_path(path)
        .with_baseline_key(baseline_key);
        if let Some(range) = span.source_range() {
            finding = finding.with_range(range);
        }
        finding
    }

    fn analysis_failure(
        path: &Path,
        kind: AnalysisFailureKind,
        message: impl Into<String>,
    ) -> Violation {
        Violation::new(message.into())
            .with_path(Arc::<Path>::from(path))
            .as_analysis_failure(kind)
    }
}
fn legacy_github_step_selector(selector: &str) -> bool {
    let Some(rest) = selector.strip_prefix("github:jobs.") else {
        return false;
    };
    let Some((_, index)) = rest.rsplit_once(".steps[") else {
        return false;
    };
    index
        .strip_suffix("].uses")
        .is_some_and(|index| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()))
}

#[derive(Debug, Clone, Copy)]
struct FindingSpan {
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

impl FindingSpan {
    fn from_yaml(span: Span) -> Self {
        // saphyr-parser documents columns as one-based but initializes them at zero and formats
        // diagnostics with `col + 1`; normalize to alint's one-based SourceRange contract.
        Self {
            start_line: span.start.line(),
            start_column: span.start.col() + 1,
            end_line: span.end.line(),
            end_column: span.end.col() + 1,
        }
    }

    fn source_range(self) -> Option<SourceRange> {
        SourceRange::new(
            self.start_line,
            self.start_column,
            self.end_line,
            self.end_column,
        )
    }
}

fn detect_format(path: &Path) -> Option<InputFormat> {
    let slash = crate::slash(path);
    let name = path.file_name()?.to_str()?;
    if (slash.starts_with(".github/workflows/") || slash.contains("/.github/workflows/"))
        && matches_extension(name, &["yml", "yaml"])
    {
        return Some(InputFormat::GithubActions);
    }
    if matches!(name, "action.yml" | "action.yaml") {
        return Some(InputFormat::GithubActions);
    }
    if matches!(name, ".pre-commit-config.yaml" | ".pre-commit-config.yml") {
        return Some(InputFormat::PreCommit);
    }
    if (slash == ".circleci/config.yml" || slash == ".circleci/config.yaml")
        || slash.ends_with("/.circleci/config.yml")
        || slash.ends_with("/.circleci/config.yaml")
    {
        return Some(InputFormat::Circleci);
    }
    if matches!(name, ".gitlab-ci.yml" | ".gitlab-ci.yaml") {
        return Some(InputFormat::GitlabCi);
    }
    if matches!(name, "cloudbuild.yml" | "cloudbuild.yaml") {
        return Some(InputFormat::CloudBuild);
    }
    if name == "Dockerfile" || name.starts_with("Dockerfile.") {
        return Some(InputFormat::Dockerfile);
    }
    None
}

fn matches_extension(name: &str, extensions: &[&str]) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extensions.contains(&extension))
}

#[derive(Debug)]
struct YamlDocument {
    nodes: Vec<YamlNode>,
    anchors: HashMap<usize, NodeId>,
    root: NodeId,
}

type NodeId = usize;
#[derive(Debug, Clone, Copy)]
struct YamlRef {
    id: NodeId,
    use_span: Option<Span>,
}

impl YamlRef {
    const fn direct(id: NodeId) -> Self {
        Self { id, use_span: None }
    }
}

#[derive(Debug, Clone, Copy)]
struct MappingEntry {
    key: YamlRef,
    value: YamlRef,
}

#[derive(Debug)]
struct YamlNode {
    kind: YamlNodeKind,
    span: Span,
    plain_scalar: bool,
    explicit_string: bool,
    gitlab_reference: bool,
}

#[derive(Debug)]
enum YamlNodeKind {
    Scalar(String),
    Sequence(Vec<NodeId>),
    Mapping(Vec<(NodeId, NodeId)>),
    Alias(usize),
}

#[derive(Clone, Copy)]
struct ScalarRef<'a> {
    value: &'a str,
    span: Span,
}

impl YamlDocument {
    fn parse(text: &str) -> std::result::Result<Option<Self>, String> {
        if !alint_core::yaml_depth::flow_depth_within_limit(text) {
            return Err("flow collection nesting exceeds the safety limit".to_owned());
        }
        if !alint_core::yaml_depth::expansion_within_limit(text) {
            return Err("alias expansion exceeds the safety limit".to_owned());
        }
        let mut parser = Parser::new_from_str(text);
        expect_event(
            &mut parser,
            |event| matches!(event, Event::StreamStart),
            "stream start",
        )?;
        if peek_matches(&mut parser, |event| matches!(event, Event::StreamEnd))? {
            let _ = next_event(&mut parser)?;
            return Ok(None);
        }
        expect_event(
            &mut parser,
            |event| matches!(event, Event::DocumentStart(_)),
            "document start",
        )?;
        let mut document = Self {
            nodes: Vec::new(),
            anchors: HashMap::new(),
            root: 0,
        };
        document.root = document.parse_node(&mut parser, 0)?;
        expect_event(
            &mut parser,
            |event| matches!(event, Event::DocumentEnd),
            "document end",
        )?;
        let (event, _) = next_event(&mut parser)?;
        match event {
            Event::StreamEnd => Ok(Some(document)),
            Event::DocumentStart(_) => Err("multiple YAML documents are not supported".to_owned()),
            _ => Err("expected stream end".to_owned()),
        }
    }

    fn parse_node<'a>(
        &mut self,
        parser: &mut Parser<'a, StrInput<'a>>,
        depth: usize,
    ) -> std::result::Result<NodeId, String> {
        if depth > MAX_YAML_NESTING {
            return Err("YAML nesting exceeds the safety limit".to_owned());
        }
        let (event, start_span) = next_event(parser)?;
        let (kind, span, anchor, plain_scalar, explicit_string, gitlab_reference) = match event {
            Event::Scalar(value, style, anchor, tag) => (
                YamlNodeKind::Scalar(value.into_owned()),
                start_span,
                anchor,
                style == ScalarStyle::Plain,
                is_explicit_string_tag(tag.as_deref()),
                is_gitlab_reference_tag(tag.as_deref()),
            ),
            Event::Alias(anchor) => (
                YamlNodeKind::Alias(anchor),
                start_span,
                0,
                false,
                false,
                false,
            ),
            Event::SequenceStart(anchor, tag) => {
                let gitlab_reference = is_gitlab_reference_tag(tag.as_deref());
                let (items, end_span) = self.parse_sequence(parser, depth + 1)?;
                (
                    YamlNodeKind::Sequence(items),
                    Span::new(start_span.start, end_span.end),
                    anchor,
                    false,
                    false,
                    gitlab_reference,
                )
            }
            Event::MappingStart(anchor, tag) => {
                let gitlab_reference = is_gitlab_reference_tag(tag.as_deref());
                let (entries, end_span) = self.parse_mapping(parser, depth + 1)?;
                (
                    YamlNodeKind::Mapping(entries),
                    Span::new(start_span.start, end_span.end),
                    anchor,
                    false,
                    false,
                    gitlab_reference,
                )
            }
            _ => return Err("expected a YAML node".to_owned()),
        };
        let id = self.nodes.len();
        self.nodes.push(YamlNode {
            kind,
            span,
            plain_scalar,
            explicit_string,
            gitlab_reference,
        });
        if anchor != 0 {
            self.anchors.insert(anchor, id);
        }
        Ok(id)
    }

    fn parse_sequence<'a>(
        &mut self,
        parser: &mut Parser<'a, StrInput<'a>>,
        depth: usize,
    ) -> std::result::Result<(Vec<NodeId>, Span), String> {
        let mut items = Vec::new();
        loop {
            if peek_matches(parser, |event| matches!(event, Event::SequenceEnd))? {
                let (_, span) = next_event(parser)?;
                return Ok((items, span));
            }
            items.push(self.parse_node(parser, depth)?);
        }
    }

    fn parse_mapping<'a>(
        &mut self,
        parser: &mut Parser<'a, StrInput<'a>>,
        depth: usize,
    ) -> std::result::Result<(Vec<(NodeId, NodeId)>, Span), String> {
        let mut entries = Vec::new();
        loop {
            if peek_matches(parser, |event| matches!(event, Event::MappingEnd))? {
                let (_, span) = next_event(parser)?;
                return Ok((entries, span));
            }
            let key = self.parse_node(parser, depth)?;
            let value = self.parse_node(parser, depth)?;
            entries.push((key, value));
        }
    }
    fn is_merge_entry(&self, key: YamlRef, value: YamlRef) -> bool {
        let Some(key) = self.resolved(key) else {
            return false;
        };
        if !self.nodes[key.id].plain_scalar
            || self.nodes[key.id].explicit_string
            || !matches!(&self.nodes[key.id].kind, YamlNodeKind::Scalar(text) if text == "<<")
        {
            return false;
        }
        if self.raw_mapping(value).is_some() {
            return true;
        }
        self.raw_sequence(value).is_some_and(|(items, sequence)| {
            items.iter().all(|item| {
                self.raw_mapping(YamlRef {
                    id: *item,
                    use_span: sequence.use_span,
                })
                .is_some()
            })
        })
    }

    fn root(&self) -> YamlRef {
        YamlRef::direct(self.root)
    }

    fn resolved(&self, mut reference: YamlRef) -> Option<YamlRef> {
        for _ in 0..=MAX_YAML_NESTING {
            match &self.nodes.get(reference.id)?.kind {
                YamlNodeKind::Alias(anchor) => {
                    reference
                        .use_span
                        .get_or_insert(self.nodes[reference.id].span);
                    reference.id = *self.anchors.get(anchor)?;
                }
                _ => return Some(reference),
            }
        }
        None
    }

    fn scalar(&self, reference: YamlRef) -> Option<ScalarRef<'_>> {
        let reference = self.resolved(reference)?;
        let YamlNodeKind::Scalar(value) = &self.nodes[reference.id].kind else {
            return None;
        };
        Some(ScalarRef {
            value,
            span: reference.use_span.unwrap_or(self.nodes[reference.id].span),
        })
    }

    fn is_yaml_null(&self, reference: YamlRef) -> bool {
        let Some(reference) = self.resolved(reference) else {
            return false;
        };
        let YamlNodeKind::Scalar(value) = &self.nodes[reference.id].kind else {
            return false;
        };
        self.nodes[reference.id].plain_scalar
            && !self.nodes[reference.id].explicit_string
            && (matches!(value.as_str(), "" | "~") || value.eq_ignore_ascii_case("null"))
    }

    fn raw_sequence(&self, reference: YamlRef) -> Option<(&[NodeId], YamlRef)> {
        let reference = self.resolved(reference)?;
        let YamlNodeKind::Sequence(items) = &self.nodes[reference.id].kind else {
            return None;
        };
        Some((items, reference))
    }

    fn sequence(&self, reference: YamlRef) -> Option<Vec<YamlRef>> {
        let (items, sequence) = self.raw_sequence(reference)?;
        Some(
            items
                .iter()
                .map(|id| YamlRef {
                    id: *id,
                    use_span: sequence.use_span,
                })
                .collect(),
        )
    }

    fn raw_mapping(&self, reference: YamlRef) -> Option<(&[(NodeId, NodeId)], YamlRef)> {
        let reference = self.resolved(reference)?;
        let YamlNodeKind::Mapping(entries) = &self.nodes[reference.id].kind else {
            return None;
        };
        Some((entries, reference))
    }

    fn effective_mapping(&self, reference: YamlRef) -> Option<Vec<MappingEntry>> {
        self.effective_mapping_inner(reference, 0)
    }

    fn effective_mapping_inner(
        &self,
        reference: YamlRef,
        depth: usize,
    ) -> Option<Vec<MappingEntry>> {
        if depth > MAX_YAML_NESTING {
            return None;
        }
        let (raw, mapping) = self.raw_mapping(reference)?;
        let mut entries = Vec::new();
        let mut positions: HashMap<&str, usize> = HashMap::new();

        // YAML merge sequences are first-wins: earlier mappings override later mappings.
        for (key_id, value_id) in raw {
            let key = YamlRef {
                id: *key_id,
                use_span: mapping.use_span,
            };
            let value = YamlRef {
                id: *value_id,
                use_span: mapping.use_span,
            };
            if !self.is_merge_entry(key, value) {
                continue;
            }
            let merge = value;
            let sources = if self.raw_mapping(merge).is_some() {
                vec![merge]
            } else {
                self.sequence(merge).unwrap_or_default()
            };
            for source in sources {
                for entry in self.effective_mapping_inner(source, depth + 1)? {
                    let name = self.scalar(entry.key)?.value;
                    if !positions.contains_key(name) {
                        positions.insert(name, entries.len());
                        entries.push(entry);
                    }
                }
            }
        }

        // Explicit keys override every merge regardless of source order. Repeated explicit keys
        // use the last value, matching the existing structured YAML pipeline.
        for (key_id, value_id) in raw {
            let key = YamlRef {
                id: *key_id,
                use_span: mapping.use_span,
            };
            let value = YamlRef {
                id: *value_id,
                use_span: mapping.use_span,
            };
            let name = self.scalar(key)?.value;
            if self.is_merge_entry(key, value) {
                continue;
            }
            let entry = MappingEntry { key, value };
            if let Some(position) = positions.get(name).copied() {
                entries[position] = entry;
            } else {
                positions.insert(name, entries.len());
                entries.push(entry);
            }
        }
        Some(entries)
    }

    fn get(&self, reference: YamlRef, wanted: &str) -> Option<YamlRef> {
        self.get_inner(reference, wanted, 0)
    }

    fn get_inner(&self, reference: YamlRef, wanted: &str, depth: usize) -> Option<YamlRef> {
        if depth > MAX_YAML_NESTING {
            return None;
        }
        let (entries, mapping) = self.raw_mapping(reference)?;
        for (key_id, value_id) in entries.iter().rev() {
            let key = YamlRef {
                id: *key_id,
                use_span: mapping.use_span,
            };
            let value = YamlRef {
                id: *value_id,
                use_span: mapping.use_span,
            };
            if !self.is_merge_entry(key, value)
                && self.scalar(key).is_some_and(|key| key.value == wanted)
            {
                return Some(value);
            }
        }
        for (key_id, value_id) in entries {
            let key = YamlRef {
                id: *key_id,
                use_span: mapping.use_span,
            };
            let merge = YamlRef {
                id: *value_id,
                use_span: mapping.use_span,
            };
            if !self.is_merge_entry(key, merge) {
                continue;
            }
            if let Some(found) = self.get_inner(merge, wanted, depth + 1) {
                return Some(found);
            }
            if let Some((sources, sequence)) = self.raw_sequence(merge) {
                for source in sources {
                    let source = YamlRef {
                        id: *source,
                        use_span: sequence.use_span,
                    };
                    if let Some(found) = self.get_inner(source, wanted, depth + 1) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    fn resolve_gitlab_reference(
        &self,
        reference: YamlRef,
    ) -> std::result::Result<Option<YamlRef>, &'static str> {
        let reference = self
            .resolved(reference)
            .ok_or("unresolved YAML alias in GitLab configuration")?;
        if !self.nodes[reference.id].gitlab_reference {
            return Ok(Some(reference));
        }
        self.resolve_gitlab_reference_inner(reference, 0, &mut Vec::new())
    }

    fn resolve_gitlab_reference_inner(
        &self,
        reference: YamlRef,
        depth: usize,
        stack: &mut Vec<NodeId>,
    ) -> std::result::Result<Option<YamlRef>, &'static str> {
        if depth > MAX_YAML_NESTING {
            return Err("GitLab !reference nesting exceeds the safety limit");
        }
        let reference = self
            .resolved(reference)
            .ok_or("unresolved YAML alias in GitLab !reference")?;
        if !self.nodes[reference.id].gitlab_reference {
            return Ok(Some(reference));
        }
        if stack.contains(&reference.id) {
            return Err("cyclic GitLab !reference");
        }
        stack.push(reference.id);
        let (segments, _) = self
            .raw_sequence(reference)
            .ok_or("GitLab !reference must contain a path sequence")?;
        if segments.is_empty() {
            return Err("GitLab !reference path is empty");
        }
        let mut target = self.root();
        for segment in segments {
            let name = self
                .scalar(YamlRef::direct(*segment))
                .ok_or("GitLab !reference path segment is not a scalar")?
                .value;
            let Some(found) = self.get(target, name) else {
                stack.pop();
                return if self.get(self.root(), "include").is_some() {
                    Ok(None)
                } else {
                    Err("GitLab !reference target does not exist")
                };
            };
            target = found;
        }
        let target = self.resolve_gitlab_reference_inner(target, depth + 1, stack)?;
        stack.pop();
        Ok(target)
    }

    fn gitlab_job_references(
        &self,
        job: YamlRef,
    ) -> std::result::Result<(Option<YamlRef>, Option<YamlRef>), &'static str> {
        self.gitlab_job_references_inner(job, 0, &mut Vec::new())
    }

    fn gitlab_job_references_inner(
        &self,
        job: YamlRef,
        depth: usize,
        stack: &mut Vec<NodeId>,
    ) -> std::result::Result<(Option<YamlRef>, Option<YamlRef>), &'static str> {
        if depth > MAX_GITLAB_EXTENDS_DEPTH {
            return Err("GitLab extends nesting exceeds the supported limit");
        }
        let job = self
            .resolved(job)
            .ok_or("unresolved YAML alias in GitLab job")?;
        if self.raw_mapping(job).is_none() {
            return Err("GitLab job is not a mapping");
        }
        if stack.contains(&job.id) {
            return Err("cyclic GitLab extends configuration");
        }
        stack.push(job.id);

        let mut inherited = (None, None);
        if let Some(extends) = self.get(job, "extends")
            && let Some(extends) = self.resolve_gitlab_reference(extends)?
        {
            if let Some(parent) = self.scalar(extends) {
                self.merge_gitlab_parent(parent.value, depth, stack, &mut inherited)?;
            } else if let Some(parents) = self.sequence(extends) {
                for parent in parents {
                    let parent = self
                        .scalar(parent)
                        .ok_or("GitLab extends parent is not a scalar")?;
                    self.merge_gitlab_parent(parent.value, depth, stack, &mut inherited)?;
                }
            } else {
                return Err("GitLab extends must be a scalar or sequence");
            }
        }

        let own_image = self.get(job, "image");
        let own_services = self.get(job, "services");
        stack.pop();
        Ok((own_image.or(inherited.0), own_services.or(inherited.1)))
    }

    fn merge_gitlab_parent(
        &self,
        name: &str,
        depth: usize,
        stack: &mut Vec<NodeId>,
        inherited: &mut (Option<YamlRef>, Option<YamlRef>),
    ) -> std::result::Result<(), &'static str> {
        let Some(parent) = self.get(self.root(), name) else {
            return if self.get(self.root(), "include").is_some() {
                Ok(())
            } else {
                Err("GitLab extends parent does not exist")
            };
        };
        let parent = self.gitlab_job_references_inner(parent, depth + 1, stack)?;
        if parent.0.is_some() {
            inherited.0 = parent.0;
        }
        if parent.1.is_some() {
            inherited.1 = parent.1;
        }
        Ok(())
    }
}

fn next_event<'a>(
    parser: &mut Parser<'a, StrInput<'a>>,
) -> std::result::Result<(Event<'a>, Span), String> {
    parser
        .next_event()
        .ok_or_else(|| "unexpected end of YAML event stream".to_owned())?
        .map_err(|error| yaml_scan_error(&error))
}
fn is_explicit_string_tag(tag: Option<&Tag>) -> bool {
    tag.is_some_and(|tag| tag.is_yaml_core_schema() && tag.suffix == "str")
}

fn is_gitlab_reference_tag(tag: Option<&Tag>) -> bool {
    tag.is_some_and(|tag| tag.handle == "!" && tag.suffix == "reference")
}

fn yaml_scan_error(error: &ScanError) -> String {
    let marker = error.marker();
    format!(
        "invalid YAML syntax at line {} column {}",
        marker.line(),
        marker.col() + 1
    )
}

fn expect_event<'a>(
    parser: &mut Parser<'a, StrInput<'a>>,
    predicate: impl FnOnce(&Event<'_>) -> bool,
    expected: &str,
) -> std::result::Result<(), String> {
    let (event, _) = next_event(parser)?;
    if predicate(&event) {
        Ok(())
    } else {
        Err(format!("expected {expected}"))
    }
}

fn peek_matches<'a>(
    parser: &mut Parser<'a, StrInput<'a>>,
    predicate: impl FnOnce(&Event<'_>) -> bool,
) -> std::result::Result<bool, String> {
    match parser.peek() {
        Some(Ok((event, _))) => Ok(predicate(event)),
        Some(Err(error)) => Err(yaml_scan_error(&error)),
        None => Err("unexpected end of YAML event stream".to_owned()),
    }
}

struct Collector<'a> {
    rule: &'a PinnedReferencesRule,
    path: Arc<Path>,
    document: &'a YamlDocument,
    findings: Vec<Violation>,
    seen_spans: HashSet<(usize, usize, usize, usize, String)>,
}

impl<'a> Collector<'a> {
    fn new(rule: &'a PinnedReferencesRule, path: &Path, document: &'a YamlDocument) -> Self {
        Self {
            rule,
            path: Arc::from(path),
            document,
            findings: Vec::new(),
            seen_spans: HashSet::new(),
        }
    }

    fn check_action(&mut self, id: YamlRef, selector: &str) {
        let Some(reference) = self.document.scalar(id) else {
            return;
        };
        if reference.value.starts_with("./") {
            return;
        }
        if let Some(image) = reference.value.strip_prefix("docker://") {
            if image == "scratch" || image_is_pinned(image) {
                return;
            }
            self.push(
                reference,
                selector,
                "container image is not pinned to a full sha256 digest",
            );
            return;
        }
        if git_reference_is_pinned(reference.value) {
            return;
        }
        self.push(
            reference,
            selector,
            "GitHub Actions reference is not pinned to a full commit SHA",
        );
    }

    fn check_image(&mut self, id: YamlRef, selector: &str) {
        let Some(reference) = self.document.scalar(id) else {
            return;
        };
        if reference.value == "scratch" || image_is_pinned(reference.value) {
            return;
        }
        self.push(
            reference,
            selector,
            "container image is not pinned to a full sha256 digest",
        );
    }

    fn check_gitlab_image_value_or_name(
        &mut self,
        id: YamlRef,
        selector: &str,
    ) -> std::result::Result<(), &'static str> {
        let Some(id) = self.document.resolve_gitlab_reference(id)? else {
            return Ok(());
        };
        let target = self.document.get(id, "name").unwrap_or(id);
        let Some(target) = self.document.resolve_gitlab_reference(target)? else {
            return Ok(());
        };
        if !self.document.is_yaml_null(target) {
            self.check_image(target, selector);
        }
        Ok(())
    }

    fn check_orb(&mut self, id: YamlRef, selector: &str) {
        let Some(reference) = self.document.scalar(id) else {
            return;
        };
        if orb_reference_is_pinned(reference.value) {
            return;
        }
        self.push(
            reference,
            selector,
            "CircleCI orb is not pinned to an exact immutable version",
        );
    }

    fn check_pre_commit_rev(&mut self, id: YamlRef, selector: &str) {
        let Some(reference) = self.document.scalar(id) else {
            return;
        };
        if is_full_git_oid(reference.value) {
            return;
        }
        self.push(
            reference,
            selector,
            "pre-commit repository is not pinned to a full commit SHA",
        );
    }

    fn missing_pre_commit_rev(&mut self, repo: ScalarRef<'_>, selector: &str) {
        self.push(
            repo,
            selector,
            "external pre-commit repository has no pinned `rev`",
        );
    }

    fn push(&mut self, reference: ScalarRef<'_>, selector: &str, message: &'static str) {
        let span = FindingSpan::from_yaml(reference.span);
        let key = (
            span.start_line,
            span.start_column,
            span.end_line,
            span.end_column,
            selector.to_owned(),
        );
        if !self.seen_spans.insert(key) {
            return;
        }
        self.findings.push(self.rule.finding(
            Arc::clone(&self.path),
            span,
            selector,
            reference.value,
            message,
        ));
    }
}

fn collect_github_actions(collector: &mut Collector<'_>) {
    let root = collector.document.root();
    if let Some(jobs) = collector.document.get(root, "jobs") {
        for entry in collector
            .document
            .effective_mapping(jobs)
            .unwrap_or_default()
        {
            let job_name = collector
                .document
                .scalar(entry.key)
                .map_or("?", |value| value.value);
            collect_github_job(collector, entry.value, job_name);
        }
    }
    if let Some(runs) = collector.document.get(root, "runs")
        && let Some(steps) = collector.document.get(runs, "steps")
    {
        collect_github_steps(collector, steps, "runs.steps");
    }
}

fn collect_github_job(collector: &mut Collector<'_>, job: YamlRef, job_name: &str) {
    if let Some(uses) = collector.document.get(job, "uses") {
        collector.check_action(uses, &format!("github:jobs.{job_name}.uses"));
    }
    if let Some(steps) = collector.document.get(job, "steps") {
        collect_github_steps(collector, steps, &format!("jobs.{job_name}.steps"));
    }
    if let Some(container) = collector.document.get(job, "container") {
        let image = collector
            .document
            .get(container, "image")
            .unwrap_or(container);
        collector.check_image(image, &format!("github:jobs.{job_name}.container.image"));
    }
    if let Some(services) = collector.document.get(job, "services") {
        for entry in collector
            .document
            .effective_mapping(services)
            .unwrap_or_default()
        {
            let name = collector
                .document
                .scalar(entry.key)
                .map_or("?", |value| value.value);
            if let Some(image) = collector.document.get(entry.value, "image") {
                collector.check_image(
                    image,
                    &format!("github:jobs.{job_name}.services.{name}.image"),
                );
            }
        }
    }
}

fn collect_github_steps(collector: &mut Collector<'_>, steps: YamlRef, prefix: &str) {
    for (index, step) in collector
        .document
        .sequence(steps)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
    {
        if let Some(uses) = collector.document.get(step, "uses") {
            collector.check_action(uses, &format!("github:{prefix}[{index}].uses"));
        }
    }
}

fn collect_pre_commit(collector: &mut Collector<'_>) {
    let Some(repos) = collector.document.get(collector.document.root(), "repos") else {
        return;
    };
    for (index, entry) in collector
        .document
        .sequence(repos)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
    {
        let Some(repo_id) = collector.document.get(entry, "repo") else {
            continue;
        };
        let Some(repo) = collector.document.scalar(repo_id) else {
            continue;
        };
        if matches!(repo.value, "local" | "meta") {
            continue;
        }
        let selector = format!("pre_commit:repos[{index}].rev");
        if let Some(rev) = collector.document.get(entry, "rev") {
            collector.check_pre_commit_rev(rev, &selector);
        } else {
            collector.missing_pre_commit_rev(repo, &selector);
        }
    }
}

fn collect_circleci(collector: &mut Collector<'_>) {
    let root = collector.document.root();
    if let Some(orbs) = collector.document.get(root, "orbs") {
        for entry in collector
            .document
            .effective_mapping(orbs)
            .unwrap_or_default()
        {
            let name = collector
                .document
                .scalar(entry.key)
                .map_or("?", |scalar| scalar.value);
            collector.check_orb(entry.value, &format!("circleci:orbs.{name}"));
        }
    }
    for section in ["jobs", "executors"] {
        let Some(records) = collector.document.get(root, section) else {
            continue;
        };
        for entry in collector
            .document
            .effective_mapping(records)
            .unwrap_or_default()
        {
            let name = collector
                .document
                .scalar(entry.key)
                .map_or("?", |scalar| scalar.value);
            collect_circleci_images(collector, entry.value, section, name);
        }
    }
}

fn collect_circleci_images(
    collector: &mut Collector<'_>,
    record: YamlRef,
    section: &str,
    name: &str,
) {
    let Some(docker) = collector.document.get(record, "docker") else {
        return;
    };
    for (index, item) in collector
        .document
        .sequence(docker)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
    {
        if let Some(image) = collector.document.get(item, "image") {
            collector.check_image(
                image,
                &format!("circleci:{section}.{name}.docker[{index}].image"),
            );
        }
    }
}

fn collect_gitlab_ci(collector: &mut Collector<'_>) -> std::result::Result<(), &'static str> {
    let root = collector.document.root();
    collect_gitlab_images_at(
        collector,
        collector.document.get(root, "image"),
        collector.document.get(root, "services"),
        "gitlab:root",
    )?;
    if let Some(default) = collector.document.get(root, "default") {
        collect_gitlab_images_at(
            collector,
            collector.document.get(default, "image"),
            collector.document.get(default, "services"),
            "gitlab:default",
        )?;
    }
    for entry in collector
        .document
        .effective_mapping(root)
        .unwrap_or_default()
    {
        let Some(name) = collector
            .document
            .scalar(entry.key)
            .map(|scalar| scalar.value)
        else {
            continue;
        };
        if name.starts_with('.')
            || gitlab_global_key(name)
            || collector.document.effective_mapping(entry.value).is_none()
        {
            continue;
        }
        let (image, services) = collector.document.gitlab_job_references(entry.value)?;
        collect_gitlab_images_at(collector, image, services, &format!("gitlab:jobs.{name}"))?;
    }
    Ok(())
}

fn collect_gitlab_images_at(
    collector: &mut Collector<'_>,
    image: Option<YamlRef>,
    services: Option<YamlRef>,
    prefix: &str,
) -> std::result::Result<(), &'static str> {
    if let Some(image) = image {
        collector.check_gitlab_image_value_or_name(image, &format!("{prefix}.image"))?;
    }
    let Some(services) = services else {
        return Ok(());
    };
    let Some(services) = collector.document.resolve_gitlab_reference(services)? else {
        return Ok(());
    };
    for (index, service) in collector
        .document
        .sequence(services)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
    {
        collector
            .check_gitlab_image_value_or_name(service, &format!("{prefix}.services[{index}]"))?;
    }
    Ok(())
}

fn gitlab_global_key(key: &str) -> bool {
    matches!(
        key,
        "after_script"
            | "before_script"
            | "cache"
            | "default"
            | "image"
            | "include"
            | "services"
            | "spec"
            | "stages"
            | "types"
            | "variables"
            | "workflow"
    )
}

fn collect_cloud_build(collector: &mut Collector<'_>) {
    let Some(steps) = collector.document.get(collector.document.root(), "steps") else {
        return;
    };
    for (index, step) in collector
        .document
        .sequence(steps)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
    {
        if let Some(name) = collector.document.get(step, "name") {
            collector.check_image(name, &format!("cloud_build:steps[{index}].name"));
        }
    }
}

fn is_full_git_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn git_reference_is_pinned(value: &str) -> bool {
    value.rsplit_once('@').is_some_and(|(locator, revision)| {
        !locator.is_empty()
            && locator.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-')
            })
            && is_full_git_oid(revision)
    })
}

fn image_is_pinned(value: &str) -> bool {
    value.rsplit_once('@').is_some_and(|(locator, digest)| {
        !locator.is_empty()
            && locator.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'.' | b'_' | b'/' | b'-' | b':' | b'[' | b']')
            })
            && sha256_is_full(digest)
    })
}

fn sha256_is_full(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

fn orb_reference_is_pinned(value: &str) -> bool {
    let Some((name, version)) = value.rsplit_once('@') else {
        return false;
    };
    let mut components = name.split('/');
    matches!(
        (
            components.next(),
            components.next(),
            components.next()
        ),
        (Some(namespace), Some(orb), None)
            if literal_orb_component(namespace) && literal_orb_component(orb)
    ) && exact_semver(version)
}

fn literal_orb_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn exact_semver(version: &str) -> bool {
    let (without_build, build) = version
        .split_once('+')
        .map_or((version, None), |(left, right)| (left, Some(right)));
    if build.is_some_and(|value| !valid_semver_suffix(value, true)) {
        return false;
    }
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(left, right)| (left, Some(right)));
    if prerelease.is_some_and(|value| !valid_semver_suffix(value, false)) {
        return false;
    }
    let mut components = core.split('.');
    matches!(
        (
            components.next(),
            components.next(),
            components.next(),
            components.next()
        ),
        (Some(major), Some(minor), Some(patch), None)
            if valid_semver_number(major)
                && valid_semver_number(minor)
                && valid_semver_number(patch)
    )
}

fn valid_semver_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value.len() == 1 || !value.starts_with('0'))
}

fn valid_semver_suffix(value: &str, build: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && (build
                    || !part.bytes().all(|byte| byte.is_ascii_digit())
                    || valid_semver_number(part))
        })
}

#[derive(Debug)]
struct DockerInstruction {
    name: String,
    arguments: String,
    span: FindingSpan,
    escape: char,
}

const MAX_DOCKERFILE_PHYSICAL_LINES: usize = 100_000;

fn dockerfile_instructions(text: &str) -> std::result::Result<Vec<DockerInstruction>, String> {
    let mut lines = Vec::new();
    for line in text.lines() {
        if lines.len() == MAX_DOCKERFILE_PHYSICAL_LINES {
            return Err(format!(
                "Dockerfile exceeds the {MAX_DOCKERFILE_PHYSICAL_LINES}-line safety limit"
            ));
        }
        lines.push(line);
    }
    let escape = docker_escape(&lines)?;
    let mut instructions = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            index += 1;
            continue;
        }
        let start_line = index + 1;
        let start_column = line.chars().count() - trimmed.chars().count() + 1;
        let mut logical = String::new();
        let mut end_line;
        loop {
            let physical = lines[index];
            let content = physical.trim_end();
            let continued = content.ends_with(escape);
            let piece = if continued {
                content.strip_suffix(escape).unwrap_or(content)
            } else {
                content
            };
            if !logical.is_empty() {
                logical.push(' ');
            }
            logical.push_str(piece.trim());
            end_line = index + 1;
            index += 1;
            if !continued {
                break;
            }
            while index < lines.len() && lines[index].trim_start().starts_with('#') {
                index += 1;
            }
            if index == lines.len() {
                return Err(format!("unterminated continuation at line {start_line}"));
            }
        }
        let (name, arguments) = split_instruction(&logical)
            .ok_or_else(|| format!("missing instruction at line {start_line}"))?;
        let heredocs = docker_heredocs(arguments, escape);
        let end_column = lines[end_line - 1].chars().count() + 1;
        instructions.push(DockerInstruction {
            name: name.to_ascii_uppercase(),
            arguments: arguments.to_owned(),
            escape,
            span: FindingSpan {
                start_line,
                start_column,
                end_line,
                end_column,
            },
        });
        skip_docker_heredocs(&lines, &mut index, &heredocs, start_line)?;
    }
    Ok(instructions)
}

/// Physical line numbers whose parsed top-level instruction is `FROM`.
///
/// `cross_file` uses this for Dockerfile `FROM` regex extracts so heredoc
/// payloads cannot impersonate an instruction while retaining the generic
/// regex capture/fix surface.
pub(crate) fn dockerfile_from_lines(text: &str) -> std::result::Result<Vec<usize>, String> {
    Ok(dockerfile_instructions(text)?
        .into_iter()
        .filter(|instruction| instruction.name == "FROM")
        .map(|instruction| instruction.span.start_line)
        .collect())
}

fn docker_escape(lines: &[&str]) -> std::result::Result<char, String> {
    let mut escape = '\\';
    let mut saw_escape = false;
    let mut saw_syntax = false;
    let mut saw_check = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        let Some(comment) = trimmed.strip_prefix('#') else {
            break;
        };
        let Some((directive, value)) = comment.split_once('=') else {
            break;
        };
        match directive.trim().to_ascii_lowercase().as_str() {
            "escape" => {
                if saw_escape {
                    return Err("duplicate escape parser directive".to_owned());
                }
                escape = match value.trim() {
                    "\\" => '\\',
                    "`" => '`',
                    _ => return Err("invalid escape parser directive".to_owned()),
                };
                saw_escape = true;
            }
            "syntax" => {
                if saw_syntax {
                    return Err("duplicate syntax parser directive".to_owned());
                }
                saw_syntax = true;
            }
            "check" => {
                if saw_check {
                    return Err("duplicate check parser directive".to_owned());
                }
                saw_check = true;
            }
            _ => break,
        }
    }
    Ok(escape)
}

fn split_instruction(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    Some(match line.find(char::is_whitespace) {
        Some(split) => (&line[..split], line[split..].trim_start()),
        None => (line, ""),
    })
}

fn docker_heredocs(arguments: &str, escape: char) -> Vec<(String, bool)> {
    let bytes = arguments.as_bytes();
    let mut delimiters = Vec::new();
    let mut quote = None;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(expected) if byte == expected => quote = None,
            Some(b'"') | None if byte == escape as u8 => {
                index += 1;
                if index < bytes.len() {
                    index += arguments[index..].chars().next().map_or(0, char::len_utf8);
                }
                continue;
            }
            None if matches!(byte, b'\'' | b'"') => quote = Some(byte),
            None if byte == b'<'
                && bytes.get(index + 1) == Some(&b'<')
                && (index == 0 || bytes[index - 1].is_ascii_whitespace()) =>
            {
                index += 2;
                let strip_tabs = bytes.get(index) == Some(&b'-');
                index += usize::from(strip_tabs);
                while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
                    index += 1;
                }
                let delimiter_quote = bytes
                    .get(index)
                    .copied()
                    .filter(|byte| matches!(byte, b'\'' | b'"'));
                index += usize::from(delimiter_quote.is_some());
                let start = index;
                while let Some(byte) = bytes.get(index) {
                    if delimiter_quote
                        .map_or_else(|| byte.is_ascii_whitespace(), |quote| *byte == quote)
                    {
                        break;
                    }
                    index += 1;
                }
                if start < index {
                    delimiters.push((arguments[start..index].to_owned(), strip_tabs));
                }
            }
            Some(_) | None => {}
        }
        index += 1;
    }
    delimiters
}

fn skip_docker_heredocs(
    lines: &[&str],
    index: &mut usize,
    heredocs: &[(String, bool)],
    instruction_line: usize,
) -> std::result::Result<(), String> {
    for (delimiter, strip_tabs) in heredocs {
        let mut found = false;
        while *index < lines.len() {
            let candidate = if *strip_tabs {
                lines[*index].trim_start_matches('\t')
            } else {
                lines[*index]
            };
            *index += 1;
            if candidate == delimiter {
                found = true;
                break;
            }
        }
        if !found {
            return Err(format!(
                "unterminated Dockerfile heredoc from line {instruction_line}"
            ));
        }
    }
    Ok(())
}

fn collect_docker_instruction(
    rule: &PinnedReferencesRule,
    path: &Arc<Path>,
    instruction: &DockerInstruction,
    ordinal: usize,
    stages: &mut HashSet<String>,
    findings: &mut Vec<Violation>,
) -> std::result::Result<(), String> {
    match instruction.name.as_str() {
        "FROM" => collect_docker_from(rule, path, instruction, ordinal, stages, findings)?,
        "ADD" => collect_docker_add(rule, path, instruction, ordinal, findings)?,
        "ONBUILD" => {
            let (name, arguments) = split_instruction(&instruction.arguments)
                .ok_or("ONBUILD requires a nested instruction")?;
            let nested = DockerInstruction {
                name: name.to_ascii_uppercase(),
                arguments: arguments.to_owned(),
                span: instruction.span,
                escape: instruction.escape,
            };
            collect_docker_instruction(rule, path, &nested, ordinal, stages, findings)?;
        }
        _ => {}
    }
    Ok(())
}

fn collect_docker_from(
    rule: &PinnedReferencesRule,
    path: &Arc<Path>,
    instruction: &DockerInstruction,
    ordinal: usize,
    stages: &mut HashSet<String>,
    findings: &mut Vec<Violation>,
) -> std::result::Result<(), String> {
    let words = docker_words(&instruction.arguments, instruction.escape)?;
    let (image_index, image) = words
        .iter()
        .enumerate()
        .find(|(_, word)| !word.value.starts_with("--"))
        .map(|(index, word)| (index, word.value.as_str()))
        .ok_or("FROM requires an image")?;
    let local_stage = stages.iter().any(|stage| stage.eq_ignore_ascii_case(image));
    if !image.eq_ignore_ascii_case("scratch") && !local_stage && !image_is_pinned(image) {
        findings.push(rule.finding(
            Arc::clone(path),
            instruction.span,
            &format!("dockerfile:instruction[{ordinal}].from"),
            image,
            "Dockerfile base image is not pinned to a full sha256 digest",
        ));
    }
    if words
        .get(image_index + 1)
        .is_some_and(|word| word.value.eq_ignore_ascii_case("as"))
        && let Some(stage) = words.get(image_index + 2)
    {
        let mut stage = stage.value.clone();
        stage.make_ascii_lowercase();
        stages.insert(stage);
    }
    Ok(())
}

fn collect_docker_add(
    rule: &PinnedReferencesRule,
    path: &Arc<Path>,
    instruction: &DockerInstruction,
    ordinal: usize,
    findings: &mut Vec<Violation>,
) -> std::result::Result<(), String> {
    let (checksum, operands) = docker_add_operands(&instruction.arguments, instruction.escape)?;
    if operands.len() < 2 {
        return Err("ADD requires at least one source and one destination".to_owned());
    }
    let checksum_is_full = checksum.is_some_and(sha256_is_full);
    for (index, source) in operands[..operands.len() - 1].iter().enumerate() {
        let remote = source.value.starts_with("http://") || source.value.starts_with("https://");
        if !source.has_variable && (!remote || checksum_is_full) {
            continue;
        }
        let message = if source.has_variable {
            "Dockerfile ADD variable source is caller-controlled"
        } else {
            "remote Dockerfile ADD source has no full sha256 checksum"
        };
        findings.push(rule.finding(
            Arc::clone(path),
            instruction.span,
            &format!("dockerfile:instruction[{ordinal}].add.source[{index}]"),
            &source.value,
            message,
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct DockerWord {
    value: String,
    has_variable: bool,
}

fn docker_add_operands(
    arguments: &str,
    escape: char,
) -> std::result::Result<(Option<&str>, Vec<DockerWord>), String> {
    let mut checksum = None;
    let mut rest = arguments.trim_start();
    while let Some(option) = rest.strip_prefix("--") {
        let end = option.find(char::is_whitespace).unwrap_or(option.len());
        let token = &option[..end];
        rest = option[end..].trim_start();
        if let Some(value) = token.strip_prefix("checksum=") {
            checksum = Some(value);
        } else if token.starts_with("from=") {
            return Err("ADD does not support --from".to_owned());
        }
    }
    let operands = if rest.starts_with('[') {
        serde_json::from_str::<Vec<String>>(rest)
            .map_err(|error| format!("invalid JSON ADD arguments: {error}"))?
            .into_iter()
            .map(|value| DockerWord {
                has_variable: has_docker_variable(&value, escape),
                value,
            })
            .collect()
    } else {
        docker_words(rest, escape)?
    };
    Ok((checksum, operands))
}

fn has_docker_variable(input: &str, escape: char) -> bool {
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == escape {
            let _ = chars.next();
        } else if ch == '$' && docker_variable_follows(&chars) {
            return true;
        }
    }
    false
}

fn docker_variable_follows(chars: &std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    chars
        .clone()
        .next()
        .is_some_and(|ch| ch == '{' || ch == '_' || ch.is_ascii_alphabetic())
}

fn docker_words(input: &str, escape: char) -> std::result::Result<Vec<DockerWord>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut has_variable = false;
    let mut chars = input.chars().peekable();
    let mut quote = None;
    while let Some(ch) = chars.next() {
        match quote {
            Some(expected) if ch == expected => quote = None,
            Some('"') if ch == escape => {
                word.push(chars.next().ok_or("trailing escape in quoted argument")?);
            }
            Some('\'') => word.push(ch),
            None if matches!(ch, '\'' | '"') => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !word.is_empty() {
                    words.push(DockerWord {
                        value: std::mem::take(&mut word),
                        has_variable: std::mem::take(&mut has_variable),
                    });
                }
            }
            None if ch == escape => {
                word.push(chars.next().ok_or("trailing escape in argument")?);
            }
            Some(_) | None => {
                has_variable |= ch == '$' && docker_variable_follows(&chars);
                word.push(ch);
            }
        }
    }
    if quote.is_some() {
        return Err("unterminated quoted argument".to_owned());
    }
    if !word.is_empty() {
        words.push(DockerWord {
            value: word,
            has_variable,
        });
    }
    Ok(words)
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: true,
    nested_rules: false,
};

pub fn options_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Options)).expect("Options JSON schema serializes")
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    if spec.paths.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "pinned_references requires a `paths` field",
        ));
    }
    if let Some(fix) = &spec.fix {
        return Err(Error::rule_config(
            &spec.id,
            format!(
                "fix.{} is not compatible with pinned_references",
                fix.op_name()
            ),
        ));
    }
    let options: Options = spec
        .deserialize_options()
        .map_err(|error| Error::rule_config(&spec.id, format!("invalid options: {error}")))?;
    Ok(Box::new(PinnedReferencesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        format: options.format,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";
    const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn run(path: &str, text: &str, format: Option<&str>) -> Vec<Violation> {
        let format = format.map_or_else(String::new, |value| format!("format: {value}\n"));
        let spec = spec_yaml(&format!(
            "id: pins\nkind: pinned_references\npaths: {path:?}\nlevel: error\n{format}"
        ));
        let rule = build(&spec).unwrap();
        let (tmp, index) = tempdir_with_files(&[(path, text.as_bytes())]);
        rule.evaluate(&ctx(tmp.path(), &index)).unwrap()
    }

    fn digest(image: &str) -> String {
        format!("{image}@sha256:{SHA256}")
    }

    #[test]
    fn pin_predicates_are_exact() {
        assert!(is_full_git_oid(SHA1));
        assert!(is_full_git_oid(SHA256));
        assert!(!is_full_git_oid(&SHA1[..39]));
        assert!(!is_full_git_oid(&format!("{}g", &SHA1[..39])));
        assert!(git_reference_is_pinned(&format!("owner/repo@{SHA1}")));
        assert!(!git_reference_is_pinned("owner/repo@v4"));
        assert!(image_is_pinned(&digest("registry:5000/image:tag")));
        assert!(image_is_pinned(&digest("[2001:db8::1]:5000/team/image")));
        assert!(!image_is_pinned("registry:5000/image:tag"));
        assert!(!image_is_pinned(&format!(
            "image@sha256:{}",
            SHA256.to_ascii_uppercase()
        )));
        assert!(!git_reference_is_pinned(&format!(
            "${{{{ matrix.action }}}}@{SHA1}"
        )));
        assert!(!image_is_pinned(&digest("$CI_IMAGE")));
        assert!(orb_reference_is_pinned("circleci/node@5.2.0"));
        assert!(orb_reference_is_pinned("owner/orb@1.2.3-rc.1+build.4"));
        for value in [
            "circleci/node@5",
            "circleci/node@5.2",
            "circleci/node@volatile",
            "circleci/node@dev:main",
            "circleci/node@1.2.x",
            "<< pipeline.parameters.orb >>@1.2.3",
        ] {
            assert!(!orb_reference_is_pinned(value), "{value}");
        }
        assert!(orb_reference_is_pinned("circleci/node@0.0.0"));
        assert!(orb_reference_is_pinned("circleci/node@1.2.3+01"));
        for value in [
            "circleci/node@01.2.3",
            "circleci/node@1.02.3",
            "circleci/node@1.2.03",
            "circleci/node@1.2.3-01",
        ] {
            assert!(!orb_reference_is_pinned(value), "{value}");
        }
    }

    #[test]
    fn templated_locators_do_not_pass_with_immutable_suffixes() {
        let action =
            format!("jobs:\n  build:\n    steps:\n      - uses: ${{{{ matrix.action }}}}@{SHA1}\n");
        assert_eq!(run(".github/workflows/ci.yml", &action, None).len(), 1);

        let image = format!("job:\n  image: $CI_IMAGE@sha256:{SHA256}\n");
        assert_eq!(run(".gitlab-ci.yml", &image, None).len(), 1);

        let orb = "version: 2.1\norbs:\n  tool: << pipeline.parameters.orb >>@1.2.3\n";
        assert_eq!(run(".circleci/config.yml", orb, None).len(), 1);
    }

    #[test]
    fn auto_detects_every_supported_path() {
        for (path, expected) in [
            (".github/workflows/ci.yml", InputFormat::GithubActions),
            ("tools/action.yaml", InputFormat::GithubActions),
            (".pre-commit-config.yaml", InputFormat::PreCommit),
            (".circleci/config.yml", InputFormat::Circleci),
            (".gitlab-ci.yml", InputFormat::GitlabCi),
            ("cloudbuild.yaml", InputFormat::CloudBuild),
            ("docker/Dockerfile.test", InputFormat::Dockerfile),
        ] {
            assert!(matches!(
                (detect_format(Path::new(path)), expected),
                (Some(InputFormat::GithubActions), InputFormat::GithubActions)
                    | (Some(InputFormat::PreCommit), InputFormat::PreCommit)
                    | (Some(InputFormat::Circleci), InputFormat::Circleci)
                    | (Some(InputFormat::GitlabCi), InputFormat::GitlabCi)
                    | (Some(InputFormat::CloudBuild), InputFormat::CloudBuild)
                    | (Some(InputFormat::Dockerfile), InputFormat::Dockerfile)
            ));
        }
        let findings = run("ci/custom.yml", "steps: []\n", None);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::Evaluation)
        );
    }

    #[test]
    fn github_covers_workflows_reusable_jobs_composites_and_images() {
        let source = format!(
            "jobs:\n  reusable:\n    uses: owner/repo/.github/workflows/ci.yml@v1\n  build:\n    container: ubuntu:24.04\n    services:\n      db:\n        image: postgres:16\n    steps:\n      - uses: actions/checkout@v4\n      - uses: ./local-action\n      - uses: owner/repo@{SHA1}\n      - uses: docker://alpine:3\n      - uses: docker://scratch\nruns:\n  using: composite\n  steps:\n    - uses: owner/another@main\n"
        );
        let findings = run(".github/workflows/ci.yml", &source, None);
        assert_eq!(findings.len(), 6);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.line)
                .collect::<Vec<_>>(),
            [Some(3), Some(5), Some(8), Some(10), Some(13), Some(18)]
        );
        assert!(findings.iter().all(|finding| finding.range.is_some()));
        assert!(
            findings
                .iter()
                .all(|finding| !finding.message.contains("owner/repo"))
        );
    }

    #[test]
    fn github_workflow_steps_preserve_the_legacy_baseline_identity() {
        let findings = run(
            ".github/workflows/ci.yml",
            "jobs:\n  build:\n    steps:\n      - uses: actions/checkout@v4\n",
            None,
        );
        assert_eq!(findings.len(), 1);
        let expected = crate::structured_path::matches_baseline_key(
            LEGACY_GITHUB_STEPS_PATH,
            LEGACY_GITHUB_PIN_REGEX,
            &serde_json::json!("actions/checkout@v4"),
        );
        assert_eq!(findings[0].baseline_key.as_deref(), Some(expected.as_str()));
    }
    #[test]
    fn github_preserves_plain_quoted_and_unicode_scalar_ranges() {
        let source = "jobs:\n  build:\n    steps:\n      - uses: 'owner/über@main'\n      - uses: owner/plain@v1\n";
        let findings = run("action.yml", source, None);
        assert_eq!(findings.len(), 2);
        let first = findings[0].range.as_ref().unwrap();
        assert_eq!(
            (
                first.start_line(),
                first.start_column(),
                first.end_line(),
                first.end_column(),
            ),
            (4, 15, 4, 32)
        );
        let second = findings[1].range.as_ref().unwrap();
        assert_eq!(
            (
                second.start_line(),
                second.start_column(),
                second.end_line(),
                second.end_column(),
            ),
            (5, 15, 5, 29)
        );
    }

    #[test]
    fn github_accepts_full_pins_and_local_references() {
        let source = format!(
            "jobs:\n  reusable:\n    uses: owner/repo/.github/workflows/ci.yml@{SHA256}\n  build:\n    container: {}\n    services:\n      db:\n        image: {}\n    steps:\n      - uses: actions/checkout@{SHA1}\n      - uses: ./local\n      - uses: docker://{}\nruns:\n  steps:\n    - uses: ./child\n",
            digest("ubuntu:24.04"),
            digest("postgres:16"),
            digest("alpine:3")
        );
        assert!(run("action.yml", &source, None).is_empty());
    }

    #[test]
    fn github_workflow_step_keeps_the_legacy_baseline_identity() {
        let findings = run(
            ".github/workflows/ci.yml",
            "jobs:\n  build:\n    steps:\n      - uses: owner/repo@main\n",
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].baseline_key.as_deref(),
            Some(
                "$.jobs.*.steps[*].uses\0=~ ^(\\./.*|docker://[^@]+@sha256:[a-f0-9]{64}|[a-zA-Z0-9._/-]+@[a-f0-9]{40})$\0got \"owner/repo@main\""
            )
        );
    }

    #[test]
    fn pre_commit_checks_revisions_and_exempts_local_repositories() {
        let source = format!(
            "repos:\n  - repo: https://example.test/one\n    rev: v1.2.3\n  - repo: https://example.test/two\n  - repo: local\n  - repo: meta\n  - repo: https://example.test/pinned\n    rev: {SHA1}\n"
        );
        let findings = run(".pre-commit-config.yaml", &source, None);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].line, Some(3));
        assert_eq!(findings[1].line, Some(4));
        assert!(findings[1].message.contains("no pinned `rev`"));
    }

    #[test]
    fn circleci_checks_exact_orbs_and_job_and_executor_images() {
        let source = "version: 2.1\norbs:\n  node: circleci/node@volatile\n  browser: circleci/browser-tools@1.4.0\njobs:\n  test:\n    docker:\n      - image: cimg/rust:1.90\nexecutors:\n  default:\n    docker:\n      - image: cimg/base:current\n";
        let findings = run(".circleci/config.yml", source, None);
        assert_eq!(findings.len(), 3);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.line)
                .collect::<Vec<_>>(),
            [Some(3), Some(8), Some(12)]
        );
    }
    #[test]
    fn gitlab_checks_root_mapping_services_and_merged_jobs_once_per_source() {
        let source = format!(
            "image: root:latest\nservices:\n  - root-service:latest\ndefault:\n  image:\n    name: ruby:3.3\n  services:\n    - name: {}\n.template: &template\n  image: alpine:latest\nbuild:\n  <<: *template\n  services:\n    - postgres:16\npinned:\n  image: {}\n",
            digest("redis:7"),
            digest("debian:bookworm")
        );
        let findings = run(".gitlab-ci.yml", &source, None);
        assert_eq!(findings.len(), 5);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.line)
                .collect::<Vec<_>>(),
            [Some(1), Some(3), Some(6), Some(12), Some(14)]
        );
    }
    #[test]
    fn explicit_keys_override_merged_mutable_values() {
        let source = format!(
            "jobs:\n  build:\n    <<: &defaults\n      container: ubuntu:latest\n    container: {}\n",
            digest("ubuntu:24.04")
        );
        assert!(run(".github/workflows/ci.yml", &source, None).is_empty());
    }

    #[test]
    fn yaml_dynamic_maps_expand_merges_and_apply_last_key_wins() {
        let source = format!(
            "definitions: &definitions\n  inherited:\n    steps:\n      - uses: owner/inherited@main\njobs:\n  <<: *definitions\n  replaced:\n    steps:\n      - uses: owner/old@main\n  replaced:\n    steps:\n      - uses: owner/new@{SHA1}\n"
        );
        let findings = run(".github/workflows/ci.yml", &source, None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(6));
    }

    #[test]
    fn yaml_container_aliases_report_each_use_site() {
        let source = "shared_steps: &shared_steps\n  - uses: owner/action@main\njob: &job\n  container: ubuntu:latest\njobs:\n  one:\n    steps: *shared_steps\n  two:\n    steps: *shared_steps\n  three: *job\n  four: *job\n";
        let findings = run(".github/workflows/ci.yml", source, None);
        assert_eq!(findings.len(), 4);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.line)
                .collect::<Vec<_>>(),
            [Some(7), Some(9), Some(10), Some(11)]
        );
    }

    #[test]
    fn yaml_merge_sequence_uses_first_mapping_then_explicit_keys() {
        let source = format!(
            "pinned: &pinned\n  container: {}\nmutable: &mutable\n  container: ubuntu:latest\njobs:\n  build:\n    <<: [*pinned, *mutable]\n",
            digest("ubuntu:24.04")
        );
        assert!(run(".github/workflows/ci.yml", &source, None).is_empty());
    }

    #[test]
    fn gitlab_pages_are_jobs_and_hidden_templates_only_fire_when_used() {
        let source = format!(
            ".template: &template\n  image: alpine:latest\nsafe:\n  <<: *template\n  image: {}\npages:\n  image: ruby:latest\n",
            digest("alpine:3")
        );
        let findings = run(".gitlab-ci.yml", &source, None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(7));
    }

    #[test]
    fn gitlab_resolves_extends_and_reference_with_source_locations() {
        let source = ".base:\n  image: alpine:latest\n  services:\n    - postgres:16\n.override:\n  image: ruby:3.3\none:\n  extends: .base\ntwo:\n  extends: [.base, .override]\nthree:\n  image: !reference [.base, image]\n  services: !reference [.base, services]\n";
        let findings = run(".gitlab-ci.yml", source, None);
        assert_eq!(findings.len(), 6);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.line)
                .collect::<Vec<_>>(),
            [Some(2), Some(2), Some(4), Some(4), Some(4), Some(6)]
        );
        assert!(findings.iter().all(|finding| finding.range.is_some()));
    }
    #[test]
    fn quoted_merge_key_remains_a_gitlab_job_name() {
        let findings = run(
            ".gitlab-ci.yml",
            "\"<<\":\n  image: alpine:latest\n  script: echo test\n",
            None,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(2));
    }

    #[test]
    fn explicit_yaml_string_tags_do_not_become_null_or_merge_keys() {
        let findings = run(
            ".gitlab-ci.yml",
            "!!str <<:\n  image: alpine:latest\njob:\n  image: !!str null\n",
            None,
        );
        assert_eq!(findings.len(), 2);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.line)
                .collect::<Vec<_>>(),
            [Some(2), Some(4)]
        );
    }

    #[test]
    fn gitlab_resolves_reference_tags_nested_in_image_mappings() {
        let source = ".images:\n  mutable: alpine:latest\n  pinned: alpine@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\njob:\n  image:\n    name: !reference [.images, mutable]\n  services:\n    - name: !reference [.images, mutable]\nsafe:\n  image:\n    name: !reference [.images, pinned]\n";
        let findings = run(".gitlab-ci.yml", source, None);
        assert_eq!(findings.len(), 2);
        assert!(
            findings
                .iter()
                .all(|finding| finding.line == Some(2) && finding.range.is_some())
        );
    }

    #[test]
    fn gitlab_include_owned_references_remain_unresolved_without_failing() {
        for source in [
            "include:\n  project: shared/pipeline\n  file: /base.yml\njob:\n  extends: .remote\n",
            "include: shared.yml\njob:\n  image: !reference [.remote, image]\n",
        ] {
            assert!(run(".gitlab-ci.yml", source, None).is_empty(), "{source:?}");
        }
    }

    #[test]
    fn gitlab_null_removes_inherited_images_and_services() {
        let source = ".base:\n  image: alpine:latest\n  services: [postgres:16]\njob:\n  extends: .base\n  image: null\n  services: ~\n";
        assert!(run(".gitlab-ci.yml", source, None).is_empty());
    }

    #[test]
    fn gitlab_inheritance_errors_fail_loud_without_values() {
        for source in [
            ".one:\n  extends: .two\n.two:\n  extends: .one\njob:\n  extends: .one\n",
            "job:\n  extends: .missing\n",
            ".base:\n  image: alpine:latest\njob:\n  image: !reference [.missing, image]\n",
        ] {
            let findings = run(".gitlab-ci.yml", source, None);
            assert_eq!(findings.len(), 1, "{source:?}");
            assert_eq!(
                findings[0].analysis_failure_kind(),
                Some(AnalysisFailureKind::ResourceParse),
                "{source:?}"
            );
            assert!(!findings[0].message.contains("alpine:latest"));
        }
    }

    #[test]
    fn duplicate_yaml_keys_use_the_last_explicit_value() {
        let source = format!(
            "job:\n  image: alpine:latest\n  image: {}\n",
            digest("alpine:3")
        );
        assert!(run(".gitlab-ci.yml", &source, None).is_empty());
    }

    #[test]
    fn cloud_build_checks_only_step_images() {
        let source = format!(
            "steps:\n  - name: gcr.io/cloud-builders/docker:latest\n  - name: {}\nimages:\n  - output/image:latest\n",
            digest("gcr.io/project/builder:tag")
        );
        let findings = run("cloudbuild.yaml", &source, None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(2));
    }

    #[test]
    fn yaml_alias_use_has_the_alias_location() {
        let source = "image: &image alpine:latest\nsteps:\n  - name: *image\n";
        let findings = run("custom.yml", source, Some("cloud_build"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(3));
        assert_eq!(findings[0].column, Some(11));
    }

    #[test]
    fn malformed_non_utf8_and_multi_document_yaml_fail_loud() {
        let findings = run(".gitlab-ci.yml", "jobs: [\n", None);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );
        let findings = run(".gitlab-ci.yml", "---\na: b\n---\nc: d\n", None);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );

        let spec =
            spec_yaml("id: pins\nkind: pinned_references\npaths: .gitlab-ci.yml\nlevel: error\n");
        let rule = build(&spec).unwrap();
        let (tmp, index) = tempdir_with_files(&[(".gitlab-ci.yml", &[0xff, 0xfe])]);
        let findings = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );
    }

    #[test]
    fn yaml_safety_limits_fail_loud_before_tree_construction() {
        let nested = format!("jobs: {}x{}\n", "[".repeat(129), "]".repeat(129));
        let findings = run(".github/workflows/ci.yml", &nested, None);
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );

        let alias_bomb = format!(
            "value: &large {}\nservices:\n{}",
            "x".repeat(1_000_000),
            "  - *large\n".repeat(300)
        );
        let findings = run(".gitlab-ci.yml", &alias_bomb, None);
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );
    }

    #[test]
    fn empty_and_comment_only_yaml_are_empty_documents() {
        assert!(run(".gitlab-ci.yml", "", None).is_empty());
        assert!(run(".gitlab-ci.yml", "# no configuration\n", None).is_empty());
    }

    #[test]
    fn parser_failures_do_not_echo_unrelated_values() {
        for (path, source) in [
            (".gitlab-ci.yml", "jobs: [TOP_SECRET\n"),
            ("Dockerfile", "RUN <<TOP_SECRET\nnever closed\n"),
        ] {
            let findings = run(path, source, None);
            assert_eq!(findings.len(), 1);
            assert_eq!(
                findings[0].analysis_failure_kind(),
                Some(AnalysisFailureKind::ResourceParse)
            );
            assert!(!findings[0].message.contains("TOP_SECRET"));
        }
    }

    #[test]
    fn oversized_inputs_fail_before_format_parsing() {
        let spec =
            spec_yaml("id: pins\nkind: pinned_references\npaths: Dockerfile\nlevel: error\n");
        let rule = build(&spec).unwrap();
        let (tmp, index) = tempdir_with_files(&[]);
        let bytes = vec![b'x'; MAX_PINNED_REFERENCES_BYTES + 1];
        let findings = rule
            .as_per_file()
            .unwrap()
            .evaluate_file(&ctx(tmp.path(), &index), Path::new("Dockerfile"), &bytes)
            .unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );
    }

    #[test]
    fn dockerfile_checks_from_and_remote_add_across_forms() {
        let source = format!(
            "# syntax=docker/dockerfile:1\nFROM --platform=$BUILDPLATFORM rust:1.90 AS build\nFROM {}\nFROM scratch\nADD https://example.test/tool /usr/bin/tool\nADD --checksum=sha256:{SHA256} https://example.test/pinned /tmp/\nADD [\"https://example.test/archive\", \"/src/\"]\nADD local.txt /src/\n",
            digest("alpine:3")
        );
        let findings = run("Dockerfile", &source, None);
        assert_eq!(findings.len(), 3);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.line)
                .collect::<Vec<_>>(),
            [Some(2), Some(5), Some(7)]
        );
        let variables = run("Dockerfile", "FROM ${BASE_IMAGE}\n", None);
        assert_eq!(variables.len(), 1);
    }

    #[test]
    fn dockerfile_honors_continuations_escape_directive_and_onbuild() {
        let source = format!(
            "# escape=`\nFROM `\n  {} AS base\nONBUILD ADD `\n  --checksum=sha256:{SHA256} `\n  https://example.test/tool /tool\nFROM ubuntu:24.04\n",
            digest("alpine:3")
        );
        let findings = run("Dockerfile.windows", &source, None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(7));
        let range = findings[0].range.as_ref().unwrap();
        assert_eq!(range.start_line(), 7);
        assert_eq!(range.end_line(), 7);
    }

    #[test]
    fn dockerfile_uses_only_the_initial_parser_directive_block() {
        let source = format!(
            "# ordinary comment\n# escape=`\nFROM \\\n  {}\n",
            digest("alpine:3")
        );
        assert!(run("Dockerfile", &source, None).is_empty());
    }

    #[test]
    fn dockerfile_accepts_prior_build_stages_and_flags_variable_add_sources() {
        let source = format!(
            "FROM {} AS build\nFROM build AS test\nADD $URL /one\nADD [\"${{ARCHIVE_URL}}\", \"/two\"]\nADD --checksum=sha256:{SHA256} $PINNED_URL /three\n",
            digest("rust:1.90")
        );
        let findings = run("Dockerfile", &source, None);
        assert_eq!(findings.len(), 3);
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.line)
                .collect::<Vec<_>>(),
            [Some(3), Some(4), Some(5)]
        );
    }

    #[test]
    fn dockerfile_stage_names_and_literal_dollars_follow_consumer_semantics() {
        let source = format!(
            "FROM {} AS BUILD\nFROM build\nFROM SCRATCH\nADD \\$LOCAL_ARCHIVE /src/\nADD '$LOCAL_ARCHIVE' /src/\nADD \"$REMOTE_URL\" /src/\n",
            digest("rust:1.90")
        );
        let findings = run("Dockerfile", &source, None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(6));
    }

    #[test]
    fn dockerfile_skips_heredoc_bodies_and_fails_on_unterminated_ones() {
        let source = "RUN <<EOF\nFROM hidden:latest\nADD https://hidden.test/file /tmp/\nEOF\nFROM visible:latest\n";
        let findings = run("Dockerfile", source, None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(5));

        let findings = run("Dockerfile", "RUN <<EOF\nnever closed\n", None);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );
    }

    #[test]
    fn dockerfile_rejects_excessive_physical_lines_before_collecting_them() {
        let source = "#\n".repeat(MAX_DOCKERFILE_PHYSICAL_LINES + 1);
        let error = dockerfile_instructions(&source).unwrap_err();
        assert!(error.contains("line safety limit"), "{error}");
    }

    #[test]
    fn dockerfile_honors_quoted_tab_stripped_and_multiple_heredocs() {
        let source = "RUN <<-'ONE' <<\"TWO\"\n\tFROM hidden-one:latest\n\tONE\nADD https://hidden-two.test/file /tmp/\nTWO\nFROM visible:latest\n";
        let findings = run("Dockerfile", source, None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(6));
    }
    #[test]
    fn dockerfile_directives_are_case_insensitive_and_malformed_arguments_fail_loud() {
        let source = format!(
            "# ESCAPE = `\nFROM `\n  {image}\n",
            image = digest("alpine")
        );
        assert!(run("Dockerfile", &source, None).is_empty());

        for source in [
            "FROM \"unterminated\n",
            "FROM\n",
            "ADD only-a-source\n",
            "ONBUILD\n",
            "ADD --from=build local /dest\n",
        ] {
            let findings = run("Dockerfile", source, None);
            assert_eq!(findings.len(), 1, "{source:?}");
            assert_eq!(
                findings[0].analysis_failure_kind(),
                Some(AnalysisFailureKind::ResourceParse),
                "{source:?}"
            );
        }

        for source in [
            "# escape=x\nFROM alpine:latest\n",
            "# escape=\\\n# escape=`\nFROM alpine:latest\n",
            "# syntax=docker/dockerfile:1\n# syntax=docker/dockerfile:1.1\nFROM alpine:latest\n",
        ] {
            let findings = run("Dockerfile", source, None);
            assert_eq!(findings.len(), 1, "{source:?}");
            assert_eq!(
                findings[0].analysis_failure_kind(),
                Some(AnalysisFailureKind::ResourceParse),
                "{source:?}"
            );
        }
    }

    #[test]
    fn explicit_format_and_custom_message_preserve_per_reference_findings() {
        let spec = spec_yaml(
            "id: pins\nkind: pinned_references\npaths: custom.yaml\nformat: cloud_build\nmessage: Pin it\nlevel: error\n",
        );
        let rule = build(&spec).unwrap();
        let (tmp, index) = tempdir_with_files(&[(
            "custom.yaml",
            b"steps:\n  - name: one:latest\n  - name: two:latest\n",
        )]);
        let findings = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().all(|finding| finding.message == "Pin it"));
        assert_ne!(findings[0].baseline_key, findings[1].baseline_key);
    }

    #[test]
    fn builder_rejects_missing_paths_unknown_options_and_fix() {
        assert!(build(&spec_yaml("id: p\nkind: pinned_references\nlevel: error\n")).is_err());
        assert!(
            build(&spec_yaml(
                "id: p\nkind: pinned_references\npaths: x\nunknown: true\nlevel: error\n"
            ))
            .is_err()
        );
        assert!(
            build(&spec_yaml(
                "id: p\nkind: pinned_references\npaths: x\nfix:\n  file_remove: {}\nlevel: error\n"
            ))
            .is_err()
        );
    }
}
