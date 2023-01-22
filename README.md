# jffs2-rs
* Rust implementation of jffs2 reader🦀

# TL;DR
* Extract the jffs2 image to the specified directory
```Rust
    let path = Path::new("path/to/image.jffs2");
    let output_path = Path::new("/tmp/some/where");
    extract_jffs2(path, output_path).expect("Failed to extract file");
```

* List all entries only
```Rust
    let path = Path::new("path/to/image.jffs2");
    let entries = list_jffs2(path).expect("Failed to list entries");
    println!("{:?}", entries);
```

# Current Status
* The following compression algorithms are supported:
    * ✔ JFFS2_COMPR_NONE
    * ✔ JFFS2_COMPR_ZERO
    * ✔ JFFS2_COMPR_RTIME
    * ✗ JFFS2_COMPR_RUBINMIPS (deprecated)
    * ✗ JFFS2_COMPR_COPY (never implemented!)
    * ✔ JFFS2_COMPR_DYNRUBIN
    * ✔ JFFS2_COMPR_ZLIB
    * ✔ JFFS2_COMPR_LZO
    * ✔ JFFS2_COMPR_LZMA