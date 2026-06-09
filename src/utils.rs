use core::{
    convert::Infallible,
    hash::Hash,
    ops::{ControlFlow, FromResidual, Residual, Try},
};
use std::collections::HashMap;

/// A trait to get the only element of a collection.
pub trait Only<T> {
    /// Get the only element of the collection.
    /// Returns `None` if the collection is empty or has more than one element.
    fn only(self) -> Option<T>;
}

impl<T, U> Only<U::Item> for T
where
    T: IntoIterator<IntoIter = U>,
    U: ExactSizeIterator,
{
    fn only(self) -> Option<U::Item> {
        let mut iter = self.into_iter();
        match iter.len() {
            1 => Some(iter.next().unwrap()),
            _ => None,
        }
    }
}

/// Performs a topological sort on items based on their parent relationships.
/// Returns items ordered such that parents appear before their children.
/// TODO: Can't get the lifetimes working for &str keys :(.
pub fn toposort<T, K, P, F1, F2>(
    items: impl IntoIterator<Item = T>,
    get_key: F1,
    get_parents: F2,
) -> Vec<T>
where
    K: Eq + Hash,
    P: IntoIterator<Item = K>,
    F1: Fn(&T) -> K,
    F2: Fn(&T) -> P,
{
    let items: Vec<T> = items.into_iter().collect();

    let mut item_map = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        item_map.insert(get_key(item), idx);
    }

    let mut in_degree: HashMap<usize, usize> = HashMap::new();
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();

    for (idx, item) in items.iter().enumerate() {
        in_degree.entry(idx).or_insert(0);
        for parent in get_parents(item) {
            if let Some(&parent_idx) = item_map.get(&parent) {
                let current = in_degree.entry(idx).or_insert(0);
                *current = current.saturating_add(1);
                children.entry(parent_idx).or_default().push(idx);
            }
        }
    }

    let mut queue: Vec<usize> = in_degree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(&idx, _)| idx)
        .collect();

    let mut result_indices = Vec::new();

    while let Some(current_idx) = queue.pop() {
        result_indices.push(current_idx);

        if let Some(child_list) = children.get(&current_idx) {
            for &child_idx in child_list {
                if let Some(degree) = in_degree.get_mut(&child_idx) {
                    *degree = degree.saturating_sub(1);
                    if *degree == 0 {
                        queue.push(child_idx);
                    }
                }
            }
        }
    }

    let mut items: HashMap<_, _> = items.into_iter().enumerate().collect();

    result_indices
        .into_iter()
        .map(|idx| {
            #[expect(clippy::missing_panics_doc, reason = "infallible")]
            items.remove(&idx).unwrap()
        })
        .collect()
}

pub enum ResultWithWarnings<T, E = crate::error::Error, W = Vec<String>> {
    Ok(T),
    OkWarnings(T, W),
    Err(E),
    ErrWarnings(E, W),
}

impl<T, E, W> ResultWithWarnings<T, E, W> {
    pub fn warnings(&self) -> Option<&W> {
        match self {
            ResultWithWarnings::OkWarnings(_, warnings)
            | ResultWithWarnings::ErrWarnings(_, warnings) => Some(warnings),
            _ => None,
        }
    }

    #[expect(clippy::missing_errors_doc, reason = "Acts like a Result")]
    pub fn into_result(self) -> Result<(T, Option<W>), E> {
        match self {
            Self::Ok(value) => Ok((value, None)),
            Self::OkWarnings(value, warnings) => Ok((value, Some(warnings))),
            Self::Err(error) | Self::ErrWarnings(error, _) => Err(error),
        }
    }
}

impl<T, E, W> From<Result<T, E>> for ResultWithWarnings<T, E, W> {
    fn from(value: Result<T, E>) -> Self {
        match value {
            Ok(value) => Self::Ok(value),
            Err(error) => Self::Err(error),
        }
    }
}

impl<T, E, W, F: From<E>> FromResidual<ResultWithWarnings<Infallible, E, W>>
    for ResultWithWarnings<T, F, W>
{
    fn from_residual(residual: ResultWithWarnings<Infallible, E, W>) -> Self {
        match residual {
            ResultWithWarnings::Err(error) => Self::Err(error.into()),
            ResultWithWarnings::ErrWarnings(error, warnings) => {
                Self::ErrWarnings(error.into(), warnings)
            }
        }
    }
}

impl<T, E, W, F: From<E>> FromResidual<core::result::Result<Infallible, E>>
    for ResultWithWarnings<T, F, W>
{
    fn from_residual(residual: core::result::Result<Infallible, E>) -> Self {
        match residual {
            Err(error) => Self::Err(error.into()),
        }
    }
}

impl<T, E, W> Residual<(T, Option<W>)> for ResultWithWarnings<Infallible, E, W> {
    type TryType = ResultWithWarnings<T, E, W>;
}

impl<T, E, W> Try for ResultWithWarnings<T, E, W> {
    type Output = (T, Option<W>);

    type Residual = ResultWithWarnings<Infallible, E, W>;

    fn from_output(output: Self::Output) -> Self {
        match output {
            (value, Some(warnings)) => Self::OkWarnings(value, warnings),
            (value, None) => Self::Ok(value),
        }
    }

    fn branch(self) -> ControlFlow<Self::Residual, Self::Output> {
        match self {
            Self::Ok(value) => ControlFlow::Continue((value, None)),
            Self::OkWarnings(value, warnings) => ControlFlow::Continue((value, Some(warnings))),
            Self::Err(error) => ControlFlow::Break(ResultWithWarnings::Err(error)),
            Self::ErrWarnings(error, warnings) => {
                ControlFlow::Break(ResultWithWarnings::ErrWarnings(error, warnings))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct Node {
        id: String,
        parents: Vec<String>,
    }

    impl Node {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_owned(),
                parents: Vec::new(),
            }
        }

        fn with_parents(id: &str, parents: impl AsRef<[&'static str]>) -> Self {
            Self {
                id: id.to_owned(),
                parents: parents.as_ref().iter().map(ToString::to_string).collect(),
            }
        }
    }

    #[test]
    fn toposort_empty() {
        let items: Vec<Node> = vec![];
        let result = toposort(items, |i| i.id.clone(), |i| i.parents.clone());
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn toposort_single_item() {
        let result = toposort([Node::new("a")], |n| n.id.clone(), |n| n.parents.clone());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "a");
    }

    #[test]
    fn toposort_linear_chain() {
        let result = toposort(
            [
                Node::new("a"),
                Node::with_parents("b", ["a"]),
                Node::with_parents("c", ["b"]),
            ],
            |n| n.id.clone(),
            |n| n.parents.clone(),
        );

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].id, "a");
        assert_eq!(result[1].id, "b");
        assert_eq!(result[2].id, "c");
    }

    #[test]
    fn toposort_multiple_roots() {
        let result = toposort(
            [
                Node::new("a"),
                Node::new("b"),
                Node::with_parents("c", ["a", "b"]),
            ],
            |n| n.id.clone(),
            |n| n.parents.clone(),
        );

        assert_eq!(result.len(), 3);
        assert!(result[0].id == "a" || result[0].id == "b");
        assert!(result[1].id == "a" || result[1].id == "b");
        assert_eq!(result[2].id, "c");
    }

    #[test]
    fn toposort_diamond_shape() {
        let result = toposort(
            [
                Node::new("a"),
                Node::with_parents("b", ["a"]),
                Node::with_parents("c", ["a"]),
                Node::with_parents("d", ["b", "c"]),
            ],
            |n| n.id.clone(),
            |n| n.parents.clone(),
        );

        assert_eq!(result.len(), 4);
        assert_eq!(result[0].id, "a");
        assert!(result[1].id == "b" || result[1].id == "c");
        assert!(result[2].id == "b" || result[2].id == "c");
        assert_eq!(result[3].id, "d");
    }

    #[test]
    fn toposort_ignores_external_parents() {
        let result = toposort(
            [
                Node::with_parents("b", vec!["a", "external"]),
                Node::new("a"),
            ],
            |n| n.id.clone(),
            |n| n.parents.clone(),
        );

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "a");
        assert_eq!(result[1].id, "b");
    }

    #[test]
    fn toposort_complex_graph() {
        let result = toposort(
            [
                Node::with_parents("f", ["d", "e"]),
                Node::with_parents("e", ["b", "c"]),
                Node::with_parents("d", ["b"]),
                Node::with_parents("c", ["a"]),
                Node::with_parents("b", ["a"]),
                Node::new("a"),
            ],
            |n| n.id.clone(),
            |n| n.parents.clone(),
        );

        assert_eq!(result.len(), 6);

        let pos_a = result.iter().position(|n| n.id == "a").unwrap();
        let pos_b = result.iter().position(|n| n.id == "b").unwrap();
        let pos_c = result.iter().position(|n| n.id == "c").unwrap();
        let pos_d = result.iter().position(|n| n.id == "d").unwrap();
        let pos_e = result.iter().position(|n| n.id == "e").unwrap();
        let pos_f = result.iter().position(|n| n.id == "f").unwrap();

        assert!(pos_a < pos_b);
        assert!(pos_a < pos_c);
        assert!(pos_b < pos_d);
        assert!(pos_b < pos_e);
        assert!(pos_c < pos_e);
        assert!(pos_d < pos_f);
        assert!(pos_e < pos_f);
    }
}
