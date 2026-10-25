//! The flat value form the plugin contract carries (ADR-0024, decision 1).

use std::fmt;

/// One node of a [`FlatValue`]. Containers name their children by index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Null,
    Bool(bool),
    /// A number as its source wrote it.
    Number(String),
    String(String),
    Bytes(Vec<u8>),
    /// A normalized repository path (`/`-separated, root-relative).
    Path(String),
    List(Vec<u32>),
    Record(Vec<(String, u32)>),
}

/// A row value as the plugin contract carries it. WIT has no recursive types,
/// so a nested value travels as a node list: node 0 is the root, containers
/// refer to their children by index, every child comes after its parent, and
/// every node but the root has exactly one parent. A list that breaks those
/// rules is rejected rather than repaired, because it arrives from outside the
/// engine, and the single-parent rule keeps a shared subtree from expanding
/// exponentially when the value is rebuilt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatValue {
    nodes: Vec<Node>,
}

/// Why a node list is not a valid [`FlatValue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlatValueError {
    Empty,
    MissingChild { node: usize, child: usize },
    ChildBeforeParent { node: usize, child: usize },
    SharedChild { child: usize },
    Unreachable { node: usize },
    TooManyNodes { nodes: usize, limit: usize },
    TooDeep { limit: usize },
}

impl fmt::Display for FlatValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a flat value needs at least its root node"),
            Self::MissingChild { node, child } => {
                write!(
                    f,
                    "node {node} refers to node {child}, which does not exist"
                )
            }
            Self::ChildBeforeParent { node, child } => write!(
                f,
                "node {node} refers to node {child}, which does not come after it"
            ),
            Self::SharedChild { child } => write!(f, "node {child} has more than one parent"),
            Self::Unreachable { node } => write!(f, "node {node} is not reachable from the root"),
            Self::TooManyNodes { nodes, limit } => {
                write!(f, "a flat value has {nodes} nodes; the limit is {limit}")
            }
            Self::TooDeep { limit } => write!(f, "a flat value nests deeper than {limit}"),
        }
    }
}

impl std::error::Error for FlatValueError {}

/// Bounds on a flat value accepted from outside the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatValueLimits {
    pub max_nodes: usize,
    pub max_depth: usize,
}

impl Default for FlatValueLimits {
    fn default() -> Self {
        Self {
            max_nodes: 1_000_000,
            max_depth: 256,
        }
    }
}

impl FlatValue {
    /// Validate a node list received from outside the engine.
    pub fn new(nodes: Vec<Node>, limits: FlatValueLimits) -> Result<Self, FlatValueError> {
        if nodes.is_empty() {
            return Err(FlatValueError::Empty);
        }
        if nodes.len() > limits.max_nodes {
            return Err(FlatValueError::TooManyNodes {
                nodes: nodes.len(),
                limit: limits.max_nodes,
            });
        }
        let parent_of = parents(&nodes)?;
        // Children come after parents, so one forward pass computes every depth.
        let mut depth = vec![0usize; nodes.len()];
        for node in 1..nodes.len() {
            let Some(parent) = parent_of[node] else {
                return Err(FlatValueError::Unreachable { node });
            };
            depth[node] = depth[parent] + 1;
            if depth[node] > limits.max_depth {
                return Err(FlatValueError::TooDeep {
                    limit: limits.max_depth,
                });
            }
        }
        Ok(Self { nodes })
    }

    /// A node list the engine built in pre-order itself: a tree by
    /// construction, with no limits to enforce against its own values.
    pub(crate) fn from_engine(nodes: Vec<Node>) -> Self {
        debug_assert!(
            !nodes.is_empty() && parents(&nodes).is_ok(),
            "the engine builds well-formed flat values"
        );
        Self { nodes }
    }

    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }
}

/// Each node's parent, checking that every reference points forward to an
/// existing node with no other parent.
fn parents(nodes: &[Node]) -> Result<Vec<Option<usize>>, FlatValueError> {
    let mut parent_of: Vec<Option<usize>> = vec![None; nodes.len()];
    for (node, entry) in nodes.iter().enumerate() {
        for child in children(entry) {
            let child = child as usize;
            if child >= nodes.len() {
                return Err(FlatValueError::MissingChild { node, child });
            }
            if child <= node {
                return Err(FlatValueError::ChildBeforeParent { node, child });
            }
            if parent_of[child].replace(node).is_some() {
                return Err(FlatValueError::SharedChild { child });
            }
        }
    }
    Ok(parent_of)
}

pub(crate) fn children(node: &Node) -> Box<dyn Iterator<Item = u32> + '_> {
    match node {
        Node::List(children) => Box::new(children.iter().copied()),
        Node::Record(fields) => Box::new(fields.iter().map(|(_, child)| *child)),
        _ => Box::new(std::iter::empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_node_lists_are_rejected() {
        let limits = FlatValueLimits::default();
        let reject = |nodes: Vec<Node>| FlatValue::new(nodes, limits).unwrap_err();
        assert_eq!(reject(vec![]), FlatValueError::Empty);
        assert_eq!(
            reject(vec![Node::List(vec![5])]),
            FlatValueError::MissingChild { node: 0, child: 5 }
        );
        assert_eq!(
            reject(vec![Node::List(vec![0])]),
            FlatValueError::ChildBeforeParent { node: 0, child: 0 }
        );
        assert_eq!(
            reject(vec![Node::List(vec![1, 1]), Node::Null]),
            FlatValueError::SharedChild { child: 1 }
        );
        assert_eq!(
            reject(vec![
                Node::List(vec![1]),
                Node::List(vec![2]),
                Node::Null,
                Node::Null
            ]),
            FlatValueError::Unreachable { node: 3 }
        );
        let tiny = FlatValueLimits {
            max_nodes: 1,
            max_depth: 8,
        };
        assert_eq!(
            FlatValue::new(vec![Node::Null, Node::Null], tiny).unwrap_err(),
            FlatValueError::TooManyNodes { nodes: 2, limit: 1 }
        );
        let chain: Vec<Node> = (0..4u32)
            .map(|i| Node::List(vec![i + 1]))
            .chain([Node::Null])
            .collect();
        let shallow = FlatValueLimits {
            max_nodes: 10,
            max_depth: 3,
        };
        assert_eq!(
            FlatValue::new(chain, shallow).unwrap_err(),
            FlatValueError::TooDeep { limit: 3 }
        );
        assert!(
            FlatValueError::SharedChild { child: 1 }
                .to_string()
                .contains("parent")
        );
    }
}
