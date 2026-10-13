use crate::css::Stylesheet;
use dioxus_core::{
    AttributeValue, ElementId, Template, TemplateAttribute, TemplateNode, WriteMutations,
};
use rust_motion_rasterizer::Scene;
use std::collections::HashMap;
use thiserror::Error;

pub(crate) type NodeKey = usize;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NativeDomError {
    #[error("Invalid Dioxus DOM mutation: {0}")]
    Mutation(String),
    #[error("CSS conversion failed: {0}")]
    Css(String),
    #[error("Native layout failed: {0}")]
    Layout(String),
    #[error("Native scene conversion failed: {0}")]
    Scene(String),
}

#[derive(Debug, Clone)]
pub(crate) enum NativeNodeKind {
    Root,
    Element(NativeElement),
    Text(String),
    Placeholder,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct NativeElement {
    pub tag: String,
    pub namespace: Option<String>,
    pub attributes: HashMap<String, String>,
    pub styles: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeNode {
    pub kind: NativeNodeKind,
    pub parent: Option<NodeKey>,
    pub children: Vec<NodeKey>,
    pub element_id: Option<usize>,
}

/// Renderer-owned DOM receiving Dioxus [`WriteMutations`].
#[derive(Debug)]
pub struct NativeDom {
    pub(crate) nodes: Vec<Option<NativeNode>>,
    pub(crate) root: NodeKey,
    ids: HashMap<usize, NodeKey>,
    stack: Vec<NodeKey>,
    error: Option<NativeDomError>,
}

impl Default for NativeDom {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeDom {
    pub fn new() -> Self {
        let root_node = NativeNode {
            kind: NativeNodeKind::Root,
            parent: None,
            children: Vec::new(),
            element_id: Some(0),
        };
        Self {
            nodes: vec![Some(root_node)],
            root: 0,
            ids: HashMap::from([(0, 0)]),
            stack: Vec::new(),
            error: None,
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.iter().filter(|node| node.is_some()).count()
    }

    pub fn to_scene(
        &self,
        width: u32,
        height: u32,
        stylesheet: &Stylesheet,
    ) -> Result<Scene, NativeDomError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        crate::scene::dom_to_scene(self, width, height, stylesheet)
    }

    pub(crate) fn node(&self, key: NodeKey) -> Result<&NativeNode, NativeDomError> {
        self.nodes
            .get(key)
            .and_then(Option::as_ref)
            .ok_or_else(|| NativeDomError::Mutation(format!("node {key} does not exist")))
    }

    fn node_mut(&mut self, key: NodeKey) -> Result<&mut NativeNode, NativeDomError> {
        self.nodes
            .get_mut(key)
            .and_then(Option::as_mut)
            .ok_or_else(|| NativeDomError::Mutation(format!("node {key} does not exist")))
    }

    fn create_node(&mut self, kind: NativeNodeKind, id: Option<ElementId>) -> NodeKey {
        let key = self.nodes.len();
        let element_id = id.map(|id| id.0);
        self.nodes.push(Some(NativeNode {
            kind,
            parent: None,
            children: Vec::new(),
            element_id,
        }));
        if let Some(id) = element_id {
            self.ids.insert(id, key);
        }
        key
    }

    fn clone_template_node(&mut self, template: &TemplateNode) -> NodeKey {
        match template {
            TemplateNode::Element {
                tag,
                namespace,
                attrs,
                children,
            } => {
                let mut element = NativeElement {
                    tag: (*tag).to_string(),
                    namespace: namespace.map(str::to_string),
                    ..Default::default()
                };
                for attribute in *attrs {
                    if let TemplateAttribute::Static {
                        name,
                        value,
                        namespace,
                    } = attribute
                    {
                        set_element_attribute(&mut element, name, *namespace, value);
                    }
                }
                let key = self.create_node(NativeNodeKind::Element(element), None);
                let child_keys = children
                    .iter()
                    .map(|child| self.clone_template_node(child))
                    .collect::<Vec<_>>();
                for child in &child_keys {
                    if let Some(node) = self.nodes[*child].as_mut() {
                        node.parent = Some(key);
                    }
                }
                self.nodes[key].as_mut().expect("new node").children = child_keys;
                key
            }
            TemplateNode::Text { text } => {
                self.create_node(NativeNodeKind::Text((*text).to_string()), None)
            }
            TemplateNode::Dynamic { .. } => self.create_node(NativeNodeKind::Placeholder, None),
        }
    }

    fn assign_id(&mut self, key: NodeKey, id: ElementId) -> Result<(), NativeDomError> {
        if let Some(previous) = self.ids.insert(id.0, key) {
            if previous != key {
                if let Ok(node) = self.node_mut(previous) {
                    node.element_id = None;
                }
            }
        }
        self.node_mut(key)?.element_id = Some(id.0);
        Ok(())
    }

    fn key_for_id(&self, id: ElementId) -> Result<NodeKey, NativeDomError> {
        self.ids.get(&id.0).copied().ok_or_else(|| {
            NativeDomError::Mutation(format!("Dioxus element id {} is not mounted", id.0))
        })
    }

    fn stack_root_at_path(&self, path: &[u8]) -> Result<NodeKey, NativeDomError> {
        let mut key = *self
            .stack
            .last()
            .ok_or_else(|| NativeDomError::Mutation("mutation stack is empty".into()))?;
        for index in path {
            key = *self
                .node(key)?
                .children
                .get(usize::from(*index))
                .ok_or_else(|| {
                    NativeDomError::Mutation(format!(
                        "template path {path:?} does not exist at child {index}"
                    ))
                })?;
        }
        Ok(key)
    }

    fn take_stack(&mut self, count: usize) -> Result<Vec<NodeKey>, NativeDomError> {
        if count > self.stack.len() {
            return Err(NativeDomError::Mutation(format!(
                "mutation requested {count} stack nodes but only {} exist",
                self.stack.len()
            )));
        }
        Ok(self.stack.split_off(self.stack.len() - count))
    }

    fn detach(&mut self, key: NodeKey) -> Result<(), NativeDomError> {
        let parent = self.node(key)?.parent;
        if let Some(parent) = parent {
            self.node_mut(parent)?
                .children
                .retain(|child| *child != key);
        }
        self.node_mut(key)?.parent = None;
        Ok(())
    }

    fn attach_children(
        &mut self,
        parent: NodeKey,
        children: Vec<NodeKey>,
    ) -> Result<(), NativeDomError> {
        for child in children {
            self.detach(child)?;
            self.node_mut(child)?.parent = Some(parent);
            self.node_mut(parent)?.children.push(child);
        }
        Ok(())
    }

    fn replace_key(
        &mut self,
        target: NodeKey,
        replacements: Vec<NodeKey>,
    ) -> Result<(), NativeDomError> {
        let parent = self.node(target)?.parent.ok_or_else(|| {
            NativeDomError::Mutation("cannot replace a detached or root node".into())
        })?;
        let position = self
            .node(parent)?
            .children
            .iter()
            .position(|child| *child == target)
            .ok_or_else(|| NativeDomError::Mutation("target is absent from parent".into()))?;
        for replacement in &replacements {
            self.detach(*replacement)?;
            self.node_mut(*replacement)?.parent = Some(parent);
        }
        self.node_mut(parent)?
            .children
            .splice(position..=position, replacements);
        self.remove_subtree(target);
        Ok(())
    }

    fn insert_relative(
        &mut self,
        target: NodeKey,
        nodes: Vec<NodeKey>,
        after: bool,
    ) -> Result<(), NativeDomError> {
        let parent = self.node(target)?.parent.ok_or_else(|| {
            NativeDomError::Mutation("cannot insert beside a detached node".into())
        })?;
        let mut position = self
            .node(parent)?
            .children
            .iter()
            .position(|child| *child == target)
            .ok_or_else(|| NativeDomError::Mutation("target is absent from parent".into()))?;
        if after {
            position += 1;
        }
        for key in &nodes {
            self.detach(*key)?;
            self.node_mut(*key)?.parent = Some(parent);
        }
        self.node_mut(parent)?
            .children
            .splice(position..position, nodes);
        Ok(())
    }

    fn remove_subtree(&mut self, key: NodeKey) {
        let Some(node) = self.nodes.get_mut(key).and_then(Option::take) else {
            return;
        };
        if let Some(id) = node.element_id {
            self.ids.remove(&id);
        }
        for child in node.children {
            self.remove_subtree(child);
        }
        self.stack.retain(|stacked| *stacked != key);
    }

    fn apply(&mut self, operation: impl FnOnce(&mut Self) -> Result<(), NativeDomError>) {
        if self.error.is_some() {
            return;
        }
        if let Err(error) = operation(self) {
            self.error = Some(error);
        }
    }
}

fn set_element_attribute(
    element: &mut NativeElement,
    name: &str,
    namespace: Option<&str>,
    value: &str,
) {
    let name = name.replace('_', "-");
    if namespace == Some("style") {
        element.styles.insert(name, value.to_string());
    } else {
        element.attributes.insert(name, value.to_string());
    }
}

fn attribute_to_string(value: &AttributeValue) -> Option<String> {
    match value {
        AttributeValue::Text(value) => Some(value.clone()),
        AttributeValue::Float(value) => Some(value.to_string()),
        AttributeValue::Int(value) => Some(value.to_string()),
        AttributeValue::Bool(value) => Some(value.to_string()),
        AttributeValue::None | AttributeValue::Listener(_) | AttributeValue::Any(_) => None,
    }
}

impl WriteMutations for NativeDom {
    fn append_children(&mut self, id: ElementId, m: usize) {
        self.apply(|this| {
            let parent = this.key_for_id(id)?;
            let children = this.take_stack(m)?;
            this.attach_children(parent, children)
        });
    }

    fn assign_node_id(&mut self, path: &'static [u8], id: ElementId) {
        self.apply(|this| {
            let key = this.stack_root_at_path(path)?;
            this.assign_id(key, id)
        });
    }

    fn create_placeholder(&mut self, id: ElementId) {
        if self.error.is_none() {
            let key = self.create_node(NativeNodeKind::Placeholder, Some(id));
            self.stack.push(key);
        }
    }

    fn create_text_node(&mut self, value: &str, id: ElementId) {
        if self.error.is_none() {
            let key = self.create_node(NativeNodeKind::Text(value.to_string()), Some(id));
            self.stack.push(key);
        }
    }

    fn load_template(&mut self, template: Template, index: usize, id: ElementId) {
        self.apply(|this| {
            let root = template.roots.get(index).ok_or_else(|| {
                NativeDomError::Mutation(format!("template root {index} does not exist"))
            })?;
            let key = this.clone_template_node(root);
            this.assign_id(key, id)?;
            this.stack.push(key);
            Ok(())
        });
    }

    fn replace_node_with(&mut self, id: ElementId, m: usize) {
        self.apply(|this| {
            let target = this.key_for_id(id)?;
            let replacements = this.take_stack(m)?;
            this.replace_key(target, replacements)
        });
    }

    fn replace_placeholder_with_nodes(&mut self, path: &'static [u8], m: usize) {
        self.apply(|this| {
            let replacements = this.take_stack(m)?;
            let target = this.stack_root_at_path(path)?;
            this.replace_key(target, replacements)
        });
    }

    fn insert_nodes_after(&mut self, id: ElementId, m: usize) {
        self.apply(|this| {
            let target = this.key_for_id(id)?;
            let nodes = this.take_stack(m)?;
            this.insert_relative(target, nodes, true)
        });
    }

    fn insert_nodes_before(&mut self, id: ElementId, m: usize) {
        self.apply(|this| {
            let target = this.key_for_id(id)?;
            let nodes = this.take_stack(m)?;
            this.insert_relative(target, nodes, false)
        });
    }

    fn set_attribute(
        &mut self,
        name: &'static str,
        ns: Option<&'static str>,
        value: &AttributeValue,
        id: ElementId,
    ) {
        self.apply(|this| {
            let key = this.key_for_id(id)?;
            let node = this.node_mut(key)?;
            let NativeNodeKind::Element(element) = &mut node.kind else {
                return Err(NativeDomError::Mutation(format!(
                    "cannot set attribute {name} on a non-element"
                )));
            };
            let normalized = name.replace('_', "-");
            let target = if ns == Some("style") {
                &mut element.styles
            } else {
                &mut element.attributes
            };
            if matches!(value, AttributeValue::None) {
                target.remove(&normalized);
            } else if let Some(value) = attribute_to_string(value) {
                target.insert(normalized, value);
            }
            Ok(())
        });
    }

    fn set_node_text(&mut self, value: &str, id: ElementId) {
        self.apply(|this| {
            let key = this.key_for_id(id)?;
            let node = this.node_mut(key)?;
            let NativeNodeKind::Text(text) = &mut node.kind else {
                return Err(NativeDomError::Mutation(
                    "cannot set text on a non-text node".into(),
                ));
            };
            *text = value.to_string();
            Ok(())
        });
    }

    fn create_event_listener(&mut self, _name: &'static str, _id: ElementId) {}

    fn remove_event_listener(&mut self, _name: &'static str, _id: ElementId) {}

    fn remove_node(&mut self, id: ElementId) {
        self.apply(|this| {
            let key = this.key_for_id(id)?;
            this.detach(key)?;
            this.remove_subtree(key);
            Ok(())
        });
    }

    fn push_root(&mut self, id: ElementId) {
        self.apply(|this| {
            let key = this.key_for_id(id)?;
            this.stack.push(key);
            Ok(())
        });
    }
}
