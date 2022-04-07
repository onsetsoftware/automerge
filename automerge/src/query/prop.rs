use crate::op_tree::{OpSetMetadata, OpTreeNode};
use crate::query::{binary_search_by, QueryResult, TreeQuery};
use crate::types::{Key, Op};
use std::fmt::Debug;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Prop<'a> {
    key: Key,
    pub ops: Vec<&'a Op>,
    pub ops_pos: Vec<usize>,
    pub pos: usize,
}

impl<'a> Prop<'a> {
    pub fn new(prop: usize) -> Self {
        Prop {
            key: Key::Map(prop),
            ops: vec![],
            ops_pos: vec![],
            pos: 0,
        }
    }
}

impl<'a> TreeQuery<'a> for Prop<'a> {
    fn query_node_with_metadata(
        &mut self,
        child: &'a OpTreeNode,
        m: &OpSetMetadata,
    ) -> QueryResult {
        let start = binary_search_by(child, |op| m.key_cmp(&op.key, &self.key));
        self.pos = start;
        for pos in start..child.len() {
            let op = child.get(pos).unwrap();
            if op.key != self.key {
                break;
            }
            if op.visible() {
                self.ops.push(op);
                self.ops_pos.push(pos);
            }
            self.pos += 1;
        }
        QueryResult::Finish
    }
}
