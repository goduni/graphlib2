use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use pyo3::create_exception;
use pyo3::exceptions;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3::types::PyTuple;
use pyo3::{Py, PyAny, Python};

mod hashedany;
use crate::hashedany::HashedAny;

create_exception!(graphlib2, CycleError, exceptions::PyValueError);

#[derive(Debug, Clone, Copy)]
enum NodeState {
    Active,
    Ready,
    Done,
}

#[derive(Clone)]
struct NodeInfo {
    parents: Vec<HashedAny>,
    children: Vec<HashedAny>,
    state: NodeState,
    npredecessors: usize,
}

impl Default for NodeInfo {
    fn default() -> NodeInfo {
        NodeInfo {
            parents: Vec::new(),
            children: Vec::new(),
            state: NodeState::Active,
            npredecessors: 0,
        }
    }
}

#[pyclass(module = "graphlib2", subclass)]
#[derive(Clone)]
struct TopologicalSorter {
    node2nodeinfo: HashMap<HashedAny, NodeInfo>,
    ready_nodes: Vec<HashedAny>,
    n_passed_out: usize,
    n_finished: usize,
    prepared: bool,
    // py_to_idx: HashMap<HashedAny, usize>,
}

impl TopologicalSorter {
    fn mark_node_as_done<T>(
        &mut self,
        node: &HashedAny,
        done_cb: &mut impl FnMut(&mut Self, &HashedAny, &mut T),
        cb_data: &mut T,
        py: Python,
    ) -> PyResult<()> {
        // Check that this node is ready to be marked as done and mark it
        // There is currently a remove and an insert here just to take ownership of the value
        // so that we can reference it while modifying other values
        // Maybe there's a better way?
        let parents = match self.node2nodeinfo.get_mut(node) {
            Some(mut v) => {
                match v.state {
                    NodeState::Active => {
                        return Err(exceptions::PyValueError::new_err(format!(
                            "node {} was not passed out (still not ready)",
                            node.0.as_ref(py).repr().unwrap().to_str().unwrap()
                        )))
                    }
                    NodeState::Done => {
                        return Err(exceptions::PyValueError::new_err(format!(
                            "node {} was already marked as done",
                            node.0.as_ref(py).repr().unwrap().to_str().unwrap()
                        )))
                    }
                    NodeState::Ready => v.state = NodeState::Done,
                }
                v.parents.clone()
            }
            None => {
                return Err(exceptions::PyValueError::new_err(format!(
                    "node {} was not added using add()",
                    node.0.as_ref(py).repr().unwrap().to_str().unwrap()
                )))
            }
        };
        self.n_finished += 1;
        // Find all parents and reduce their dependency count by one,
        // returning all parents w/o any further dependencies
        let mut parent_info: &mut NodeInfo;
        for parent in parents {
            parent_info = self.node2nodeinfo.get_mut(&parent).unwrap();
            parent_info.npredecessors -= 1;
            if parent_info.npredecessors == 0 {
                parent_info.state = NodeState::Ready;
                done_cb(self, &parent, cb_data);
            }
        }
        Ok(())
    }

    fn add_node(&mut self, node: HashedAny, children: Vec<HashedAny>) -> PyResult<()> {
        let mut nodeinfo = self
            .node2nodeinfo
            .entry(node.clone())
            .or_insert_with(|| NodeInfo {
                children: children.clone(),
                ..Default::default()
            });
        nodeinfo.npredecessors += children.len();
        for child in children {
            self.node2nodeinfo
                .entry(child)
                .or_insert_with(|| NodeInfo::default())
                .parents
                .push(node.clone());
        }
        Ok(())
    }
    fn find_cycle(&self) -> Option<Vec<Py<PyAny>>> {
        let mut seen = HashSet::new();
        let mut stack: Vec<&HashedAny> = Vec::new();
        let mut itstack = Vec::new();
        let mut node2stackidx = HashMap::new();

        for mut node in self.node2nodeinfo.keys() {
            // // Only begin exploring from root nodes
            // if nodeinfo.parents.len() != 0 {
            //     continue;
            // }
            if seen.contains(node) {
                continue;
            }
            'outer: loop {
                if seen.contains(node) {
                    // If this node is in the current stack, we have a cycle
                    if node2stackidx.contains_key(node) {
                        let start_idx = node2stackidx.get(node).unwrap();
                        let mut res = Vec::with_capacity(stack.len() - *start_idx);
                        for n in stack[*start_idx..].iter() {
                            res.push(n.0.clone())
                        }
                        res.push(node.0.clone());
                        return Some(res);
                    }
                } else {
                    seen.insert(node);
                    itstack.push(self.node2nodeinfo.get(node).unwrap().parents.iter());
                    node2stackidx.insert(node, stack.len());
                    stack.push(node);
                }
                // Backtrack to the topmost stack entry with at least 1 parent
                let mut broke = false;
                while !stack.is_empty() {
                    match itstack.last_mut().unwrap().next() {
                        Some(parent) => {
                            node = parent;
                            broke = true;
                            break;
                        }
                        None => {
                            node2stackidx.remove(stack.pop().unwrap());
                            itstack.pop();
                            continue;
                        }
                    }
                }
                if !broke {
                    break 'outer;
                }
            }
        }
        None
    }
}

#[pymethods]
impl TopologicalSorter {
    fn add(&mut self, node: HashedAny, predecessors: Vec<HashedAny>) -> PyResult<()> {
        self.add_node(node, predecessors)?;
        Ok(())
    }
    fn prepare(&mut self, py: Python) -> PyResult<()> {
        if self.prepared {
            return Err(exceptions::PyValueError::new_err(
                "cannot prepare() more than once",
            ));
        }
        match self.find_cycle() {
            Some(cycle) => {
                let mut items = Vec::new();
                for item in &cycle {
                    items.push(item.as_ref(py).repr()?.to_str()?);
                }
                return Err(CycleError::new_err((
                    format!("nodes are in a cycle [{}]", items.join(", ")),
                    cycle,
                )));
            }
            None => (),
        }
        self.prepared = true;
        for (node, nodeinfo) in self.node2nodeinfo.iter_mut() {
            if nodeinfo.npredecessors == 0 {
                self.ready_nodes.push(node.clone());
                nodeinfo.state = NodeState::Ready;
            }
        }
        Ok(())
    }
    #[new]
    fn new(graph: Option<&PyDict>) -> PyResult<Self> {
        let mut this = TopologicalSorter {
            node2nodeinfo: HashMap::new(),
            ready_nodes: Vec::new(),
            n_passed_out: 0,
            n_finished: 0,
            prepared: false,
        };
        if !graph.is_none() {
            for (node, v) in graph.unwrap().iter() {
                let i = v.iter()?;
                let mut children: Vec<HashedAny> = Vec::new();
                for el in i {
                    children.push(HashedAny::extract(el?)?);
                }
                this.add_node(node.extract()?, children)?;
            }
        }
        Ok(this)
    }
    /// Returns string representation of the graph
    fn __str__(&self) -> PyResult<String> {
        Ok(format!("TopologicalSorter()"))
    }
    fn __repr__(&self) -> PyResult<String> {
        self.__str__()
    }
    /// Returns a deep copy of this graph
    fn copy(&self) -> TopologicalSorter {
        self.clone()
    }
    /// Returns any nodes with no dependencies after marking `node` as done
    /// # Arguments
    ///
    /// * `node` - A node in the graph
    fn done(&mut self, nodes: Vec<HashedAny>, py: Python) -> PyResult<()> {
        if !self.prepared {
            return Err(exceptions::PyValueError::new_err(
                "prepare() must be called first",
            ));
        }
        let mut done_db =
            |s: &mut Self, done_node: &HashedAny, _: &mut ()| s.ready_nodes.push(done_node.clone());
        for node in nodes {
            self.mark_node_as_done(&node, &mut done_db, &mut (), py)?;
        }
        Ok(())
    }
    fn is_active(&self) -> PyResult<bool> {
        if !self.prepared {
            return Err(exceptions::PyValueError::new_err(
                "prepare() must be called first",
            ));
        }
        Ok(self.n_finished < self.n_passed_out || !self.ready_nodes.is_empty())
    }
    /// Returns all nodes with no dependencies
    fn get_ready<'py>(&mut self, py: Python<'py>) -> PyResult<&'py PyTuple> {
        if !self.prepared {
            return Err(exceptions::PyValueError::new_err(
                "prepare() must be called first",
            ));
        }
        let ret = PyTuple::new(py, self.ready_nodes.iter().map(|node| node.0.clone()));
        self.n_passed_out += self.ready_nodes.len();
        self.ready_nodes.clear();
        Ok(ret)
    }
    fn static_order<'py>(&mut self, py: Python<'py>) -> PyResult<Vec<Py<PyAny>>> {
        self.prepare(py)?;
        let mut out = Vec::new();
        let mut queue: VecDeque<HashedAny> = VecDeque::from(self.ready_nodes.clone());
        let mut node: HashedAny;
        let mut done_cb = |_: &mut Self, done_node: &HashedAny, q: &mut VecDeque<HashedAny>| {
            q.push_back(done_node.clone());
        };
        loop {
            if queue.is_empty() {
                break;
            }
            node = queue.pop_front().unwrap();
            self.mark_node_as_done(&node, &mut done_cb, &mut queue, py)?;
            out.push(node.0);
        }
        self.n_passed_out += out.len();
        Ok(out)
    }
}

#[pymodule]
fn graphlib2(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<TopologicalSorter>()?;
    m.add("CycleError", _py.get_type::<CycleError>())?;
    Ok(())
}
