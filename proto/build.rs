use tonic_build::configure;
fn main() {
    configure()
    .include_file("mod.rs")
        .compile(
            &[
                "proto/gossip.proto",
                "proto/publicrpc.proto",
                "proto/spy.proto",
                "proto/http.proto",
                "proto/annotations.proto"
            ],
            &["proto"],
        )
        .unwrap();
}


// fn main() -> anyhow::Result<()> {
//     tonic_build::compile_protos("../proto/proto/gossip.proto")?;
//     // tonic_build::compile_protos("../proto/proto/publicrpc.proto")?;
//     // tonic_build::compile_protos("../proto/proto/spy.proto")?;
//     tonic_build::compile_protos("../proto/proto/http.proto")?;
//     //  tonic_build::compile_protos("../proto/proto/annotations.proto")?;
//     Ok(())
// }
