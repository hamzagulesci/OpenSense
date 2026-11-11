//! GPU load from the graphics kernel's own statistics
//! (`D3DKMTQueryStatistics`), the same source Task Manager uses. Unlike the
//! PDH "GPU Engine" counters it loads nothing extra into the process.
//!
//! Load of an adapter = busiest engine (node): running-time delta divided by
//! wall-clock delta. The busiest adapter is reported.

use std::time::Instant;

use windows::Wdk::Graphics::Direct3D::{
    D3DKMTCloseAdapter, D3DKMTEnumAdapters2, D3DKMTQueryStatistics, D3DKMT_ADAPTERINFO, D3DKMT_CLOSEADAPTER,
    D3DKMT_ENUMADAPTERS2, D3DKMT_QUERYSTATISTICS, D3DKMT_QUERYSTATISTICS_ADAPTER, D3DKMT_QUERYSTATISTICS_NODE,
};
use windows::Win32::Foundation::LUID;

use crate::SysError;

struct Adapter {
    luid: LUID,
    nodes: u32,
    /// Running time per node at the previous sample (100 ns units).
    last: Vec<i64>,
}

pub struct GpuLoadMonitor {
    adapters: Vec<Adapter>,
    last_sample: Option<Instant>,
}

fn query(luid: LUID, setup: impl FnOnce(&mut D3DKMT_QUERYSTATISTICS)) -> Option<D3DKMT_QUERYSTATISTICS> {
    let mut q = D3DKMT_QUERYSTATISTICS { AdapterLuid: luid, ..Default::default() };
    setup(&mut q);
    // The kernel writes the result into `q`, so pass a pointer derived from
    // a mutable borrow.
    let status = unsafe { D3DKMTQueryStatistics(std::ptr::addr_of_mut!(q)) };
    status.is_ok().then_some(q)
}

fn node_running_time(luid: LUID, node: u32) -> Option<i64> {
    query(luid, |q| {
        q.Type = D3DKMT_QUERYSTATISTICS_NODE;
        q.Anonymous.QueryNode.NodeId = node;
    })
    .map(|q| unsafe { q.QueryResult.NodeInformation.GlobalInformation.RunningTime })
}

impl GpuLoadMonitor {
    pub fn new() -> Result<Self, SysError> {
        unsafe {
            let mut e = D3DKMT_ENUMADAPTERS2::default();
            if D3DKMTEnumAdapters2(&mut e).is_err() || e.NumAdapters == 0 {
                return Err(SysError::Win("no GPU adapters".into()));
            }
            let mut infos = vec![D3DKMT_ADAPTERINFO::default(); e.NumAdapters as usize];
            e.pAdapters = infos.as_mut_ptr();
            if D3DKMTEnumAdapters2(&mut e).is_err() {
                return Err(SysError::Win("D3DKMTEnumAdapters2 failed".into()));
            }
            infos.truncate(e.NumAdapters as usize);

            let mut adapters = Vec::new();
            for info in &infos {
                let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: info.hAdapter });
                let nodes = query(info.AdapterLuid, |q| q.Type = D3DKMT_QUERYSTATISTICS_ADAPTER)
                    .map(|q| q.QueryResult.AdapterInformation.NodeCount)
                    .unwrap_or(0);
                if nodes > 0 {
                    let last = (0..nodes).map(|n| node_running_time(info.AdapterLuid, n).unwrap_or(0)).collect();
                    adapters.push(Adapter { luid: info.AdapterLuid, nodes, last });
                }
            }
            Ok(Self { adapters, last_sample: Some(Instant::now()) })
        }
    }

    /// Load (0..=100) of the busiest adapter since the previous call.
    pub fn sample(&mut self) -> Result<Option<f32>, SysError> {
        let now = Instant::now();
        let Some(prev) = self.last_sample.replace(now) else { return Ok(None) };
        let wall = now.duration_since(prev).as_nanos() as f64 / 100.0;
        if wall <= 0.0 {
            return Ok(None);
        }
        let mut busiest = 0.0f64;
        for a in &mut self.adapters {
            for n in 0..a.nodes {
                // A failed query keeps the last value: storing 0 would turn
                // the next good sample into a 100 % spike.
                let Some(t) = node_running_time(a.luid, n) else { continue };
                let delta = (t - a.last[n as usize]).max(0) as f64;
                a.last[n as usize] = t;
                busiest = busiest.max(delta / wall);
            }
        }
        Ok(Some((busiest * 100.0).clamp(0.0, 100.0) as f32))
    }
}
