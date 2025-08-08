use host_integration::{config::HostMempoolConfig, node::TestNode};

pub fn create_config(idx: usize, count: usize, base_port: u16) -> HostMempoolConfig {
    let mut config = HostMempoolConfig::default();
    config.p2p.listen_addr = format!("/ip4/127.0.0.1/tcp/{}", base_port + idx as u16)
        .parse()
        .unwrap();
    for i in 0..count {
        if i == idx {
            continue;
        }
        config.p2p.persistent_peers.push(
            format!("/ip4/127.0.0.1/tcp/{}", base_port + i as u16)
                .parse()
                .unwrap(),
        );
    }

    config
}

pub async fn create_nodes(count: usize, base_port: u16) -> Vec<TestNode> {
    // Create test nodes with different ports
    let mut nodes: Vec<TestNode> = Vec::new();
    for i in 0..count {
        let config = create_config(i, count, base_port);
        nodes.push(TestNode::new(i, config).await);
        println!("Created node {i}");
    }

    nodes
}
