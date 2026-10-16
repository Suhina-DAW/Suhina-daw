// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Authoring geometry; processing connections remain the authority for signal routing.
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{Connection, DeviceGraph, DeviceSdkError, MAX_CONNECTIONS, MAX_NODES};

pub const MAX_WIRE_POINTS: usize = 256;
pub const MAX_TOTAL_WIRE_POINTS: usize = 4096;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Editor {
    pub graph: GraphLayout,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GraphLayout {
    pub nodes: Vec<NodeLayout>,
    pub wires: Vec<WireLayout>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLayout {
    pub node: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireLayout {
    pub connection: Connection,
    /// Intermediate bends in graph coordinates; endpoints are port anchors.
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    /// Offset from the source node's top-left, or automatic port placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_anchor: Option<[f32; 2]>,
    /// Offset from the destination node's top-left, or automatic port placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_anchor: Option<[f32; 2]>,
}

impl WireLayout {
    pub fn new(connection: Connection) -> Self {
        Self { connection, points: Vec::new(), from_anchor: None, to_anchor: None }
    }
}

impl Editor {
    pub(crate) fn validate(&self, device: &DeviceGraph) -> Result<(), DeviceSdkError> {
        let fail = |message: &str| DeviceSdkError(format!("editor graph: {message}"));
        if self.graph.nodes.len() > MAX_NODES || self.graph.wires.len() > MAX_CONNECTIONS {
            return Err(fail("layout count exceeds graph limits"));
        }
        let mut nodes = HashMap::new();
        for layout in &self.graph.nodes {
            if !device.nodes.iter().any(|n| n.id == layout.node) || nodes.insert(layout.node.as_str(), layout).is_some()
            {
                return Err(fail(&format!("missing or duplicate node layout {:?}", layout.node)));
            }
            if !point_valid(layout.position)
                || layout.size.iter().any(|v| !v.is_finite() || !(24.0..=4096.0).contains(v))
            {
                return Err(fail(&format!("invalid geometry for node {:?}", layout.node)));
            }
        }
        let mut wires = HashSet::new();
        let mut anchors = HashMap::new();
        let mut total_points = 0usize;
        for wire in &self.graph.wires {
            let c = &wire.connection;
            if !device.connections.contains(c) || !wires.insert((&c.from, &c.from_port, &c.to, &c.to_port)) {
                return Err(fail(&format!("missing or duplicate connection route {c:?}")));
            }
            total_points = total_points.checked_add(wire.points.len()).ok_or_else(|| fail("too many bends"))?;
            if wire.points.len() > MAX_WIRE_POINTS
                || total_points > MAX_TOTAL_WIRE_POINTS
                || wire.points.iter().any(|p| !point_valid(*p))
            {
                return Err(fail("wire bends exceed geometry/count limits"));
            }
            for (node, port, output, anchor) in
                [(&c.from, &c.from_port, true, wire.from_anchor), (&c.to, &c.to_port, false, wire.to_anchor)]
            {
                let Some(anchor) = anchor else { continue };
                let Some(layout) = nodes.get(node.as_str()) else {
                    return Err(fail("explicit port anchor requires a node layout"));
                };
                if anchor.iter().zip(layout.size).any(|(v, size)| !v.is_finite() || !(0.0..=size).contains(v)) {
                    return Err(fail(&format!("port anchor outside node {node:?}")));
                }
                if anchors.insert((node, port, output), anchor).is_some_and(|previous| previous != anchor) {
                    return Err(fail(&format!("inconsistent anchor for port {node:?}/{port:?}")));
                }
            }
        }
        Ok(())
    }
}

fn point_valid(point: [f32; 2]) -> bool {
    point.iter().all(|v| v.is_finite() && (-8192.0..=8192.0).contains(v))
}
