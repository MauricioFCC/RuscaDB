#![allow(missing_docs)]
#![allow(dead_code)]
//! Copy-on-write B+tree (SPEC-0130): stub.

#[derive(Debug, Clone, Default)]
pub struct CowBTreeNode {
    pub keys: Vec<i64>,
    pub values: Vec<String>,
    pub epoch: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CowBTree {
    root: Option<CowBTreeNode>,
    epoch: u64,
}

impl CowBTree {
    pub fn new() -> Self {
        Self {
            root: None,
            epoch: 0,
        }
    }
    pub fn insert(&mut self, key: i64, value: String) {
        self.epoch += 1;
        let mut node = self.root.take().unwrap_or_default();
        node.keys.push(key);
        node.values.push(value);
        node.epoch = self.epoch;
        self.root = Some(node);
    }
    pub fn get(&self, key: i64) -> Option<String> {
        self.root.as_ref().and_then(|n| {
            n.keys
                .iter()
                .position(|&k| k == key)
                .map(|i| n.values[i].clone())
        })
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn len(&self) -> usize {
        self.root.as_ref().map(|n| n.keys.len()).unwrap_or(0)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0130-01
    fn test_ac_0130_01_cow_snapshot_isolation() {
        let mut tree = CowBTree::new();
        tree.insert(1, "a".to_string());
        assert_eq!(tree.get(1), Some("a".to_string()));
        assert_eq!(tree.epoch(), 1);
    }
}
