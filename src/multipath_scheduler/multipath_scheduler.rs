// Copyright (c) 2023 The TQUIC Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(unused_variables)]

use core::str::FromStr;
use std::time::Instant;

use self::scheduler_minrtt::*;
use self::scheduler_redundant::*;
use crate::connection::path::PathMap;
use crate::connection::space::PacketNumSpaceMap;
use crate::connection::space::SentPacket;
use crate::connection::stream::StreamMap;
use crate::Error;
use crate::MultipathConfig;
use crate::Result;

/// MultipathScheduler is a packet scheduler that decides the path over which
/// the next QUIC packet will be sent.
/// Note: The API of MultipathScheduler is not stable and may change in future
/// versions.
pub(crate) trait MultipathScheduler {
    /// Select a validated path with sufficient congestion window for sending
    /// non-probing packets.
    fn on_select(
        &mut self,
        paths: &mut PathMap,
        spaces: &mut PacketNumSpaceMap,
        streams: &mut StreamMap,
    ) -> Result<usize>;

    /// Process a packet sent event.
    fn on_sent(
        &mut self,
        packet: &SentPacket,
        now: Instant,
        path_id: usize,
        paths: &mut PathMap,
        spaces: &mut PacketNumSpaceMap,
        streams: &mut StreamMap,
    ) {
    }
}

/// Available multipath scheduling algorithm
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MultipathAlgorithm {
    /// The scheduler sends packets over the path with the lowest smoothed RTT
    /// among all available paths. It aims to optimize throughput and achieve
    /// load balancing, making it particularly advantageous for bulk transfer
    /// applications in heterogeneous networks.
    MinRtt,

    /// The scheduler sends all packets redundantly on all available paths. It
    /// utilizes additional bandwidth to minimize latency, thereby reducing the
    /// overall flow completion time for applications with bounded bandwidth
    /// requirements that can be met by a single path.
    /// In scenarios where two paths with varying available bandwidths are
    /// present, it ensures a goodput at least equivalent to the best single
    /// path.
    Redundant,
}

impl FromStr for MultipathAlgorithm {
    type Err = Error;

    fn from_str(algor: &str) -> Result<MultipathAlgorithm> {
        if algor.eq_ignore_ascii_case("minrtt") {
            Ok(MultipathAlgorithm::MinRtt)
        } else if algor.eq_ignore_ascii_case("redundant") {
            Ok(MultipathAlgorithm::Redundant)
        } else {
            Err(Error::InvalidConfig("unknown".into()))
        }
    }
}

/// Build a multipath scheduler
pub(crate) fn build_multipath_scheduler(conf: &MultipathConfig) -> Box<dyn MultipathScheduler> {
    match conf.multipath_algor {
        MultipathAlgorithm::MinRtt => Box::new(MinRttScheduler::new(conf)),
        MultipathAlgorithm::Redundant => Box::new(RedundantScheduler::new(conf)),
    }
}

pub(crate) fn reinjection_required(algor: MultipathAlgorithm) -> bool {
    match algor {
        MultipathAlgorithm::MinRtt => false,
        MultipathAlgorithm::Redundant => true,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::connection::stream;
    use crate::Config;
    use crate::Path;
    use crate::TransportParams;
    use std::time::Duration;

    pub(crate) struct MultipathTester {
        pub(crate) paths: PathMap,
        pub(crate) spaces: PacketNumSpaceMap,
        pub(crate) streams: StreamMap,
    }

    impl MultipathTester {
        /// Create context for multipath scheduler.
        pub(crate) fn new() -> Result<MultipathTester> {
            let path = new_test_path("127.0.0.1:443", "127.0.0.1:8443", true, 200);
            let mut paths = PathMap::new(path, 8, true);
            paths.enable_multipath();

            let spaces = PacketNumSpaceMap::new();

            let params = stream::StreamTransportParams::from(&TransportParams::default());
            let streams = StreamMap::new(true, 1024 * 1024, 1024 * 1024, params);

            Ok(MultipathTester {
                paths,
                spaces,
                streams,
            })
        }

        /// Add a test path.
        pub(crate) fn add_path(
            &mut self,
            local: &str,
            remote: &str,
            initial_rtt: u64,
        ) -> Result<usize> {
            let mut path = new_test_path(local, remote, false, initial_rtt);
            path.set_active(true);
            path.dcid_seq = Some(self.paths.len() as u64);
            self.paths.insert_path(path)
        }

        /// Mark the given path as active or inactive.
        pub(crate) fn set_path_active(&mut self, path_id: usize, active: bool) -> Result<()> {
            let path = self.paths.get_mut(path_id)?;
            path.set_active(active);
            Ok(())
        }
    }

    fn new_test_path(local: &str, remote: &str, is_initial: bool, initial_rtt: u64) -> Path {
        let local = local.parse().unwrap();
        let remote = remote.parse().unwrap();
        let mut conf = Config::new().unwrap();
        conf.recovery.initial_rtt = Duration::from_millis(initial_rtt);

        Path::new(local, remote, is_initial, &conf.recovery, "")
    }

    #[test]
    fn scheduler_name() {
        let cases = [
            ("minrtt", Ok(MultipathAlgorithm::MinRtt)),
            ("Minrtt", Ok(MultipathAlgorithm::MinRtt)),
            ("MinRtt", Ok(MultipathAlgorithm::MinRtt)),
            ("MINRTT", Ok(MultipathAlgorithm::MinRtt)),
            ("redundant", Ok(MultipathAlgorithm::Redundant)),
            ("Redundant", Ok(MultipathAlgorithm::Redundant)),
            ("REDUNDANT", Ok(MultipathAlgorithm::Redundant)),
            ("redun", Err(Error::InvalidConfig("unknown".into()))),
        ];

        for (name, algor) in cases {
            assert_eq!(MultipathAlgorithm::from_str(name), algor);
        }
    }
}

mod scheduler_minrtt;
mod scheduler_redundant;
