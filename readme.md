# Erasure Node
`erasure-node` crate implements erasure-coded file replication node logic.

Implementation of the communication between nodes is left to the user of the library for flexibility.
More specifically, users need to implement the `Network` trait and pass it to `Node::new(..)`.

# Simulation
`replic-sim` crate runs a simple simulation to validate the correctness of the node.

Can be configured by changing the parameters of the Config struct.
Running simulation with: `RUST_LOG=info cargo run --release`
