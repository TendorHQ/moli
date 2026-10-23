use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn run(command: &mut Command) {
    let status = command.status().expect("could not start native build tool");
    assert!(status.success(), "native build command failed: {command:?}");
}

fn unpack(archive: &str, destination: &Path) {
    let archive = fs::File::open(archive).expect("bundled native source");
    tar::Archive::new(flate2::read::GzDecoder::new(archive))
        .unpack(destination)
        .expect("unpack bundled native source");
}

fn x86_assembler(out: &Path) -> PathBuf {
    if let Ok(output) = Command::new("nasm").arg("-v").output()
        && output.status.success()
        && let Some(version) = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .nth(2)
        && let Some((major, minor)) = version.split_once('.')
        && let (Ok(major), Some(Ok(minor))) = (
            major.parse::<u32>(),
            minor.split('.').next().map(str::parse::<u32>),
        )
        && (major, minor) >= (2, 13)
    {
        return "nasm".into();
    }

    // NASM runs on the build host, even when x264 targets another architecture.
    // Its release archive contains generated sources, so Perl/autoconf are not
    // required and a clean Cargo build does not need network access.
    let source = out.join("nasm-2.16.03");
    let assembler = source.join("nasm");
    if !assembler.is_file() {
        println!("cargo:warning=NASM >= 2.13 not found; building bundled NASM 2.16.03");
        if !source.join("configure").is_file() {
            unpack("native/nasm.tar.gz", out);
        }
        let host = env::var("HOST").expect("HOST");
        let compiler = cc::Build::new().target(&host).get_compiler();
        run(Command::new("sh")
            .arg(source.join("configure"))
            .arg("--disable-debug")
            .env("CC", compiler.path())
            .current_dir(&source));
        run(Command::new("make")
            .arg("-j")
            .arg(env::var("NUM_JOBS").unwrap_or_else(|_| "1".into()))
            .arg("nasm")
            .current_dir(&source));
    }
    assembler
}

fn main() {
    println!("cargo:rerun-if-changed=native/x264.tar.gz");
    println!("cargo:rerun-if-changed=native/nasm.tar.gz");
    println!("cargo:rerun-if-changed=native/bridge.c");
    println!("cargo:rerun-if-env-changed=MOLI_X264_DIR");
    println!("cargo:rerun-if-env-changed=AS");
    println!("cargo:rerun-if-env-changed=PATH");
    let target = env::var("TARGET").expect("TARGET");
    let windows = target.contains("windows");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let (include, library, library_name) = if let Some(prefix) = env::var_os("MOLI_X264_DIR") {
        let prefix = PathBuf::from(prefix);
        assert!(
            prefix.join("include/x264.h").is_file(),
            "MOLI_X264_DIR must contain include/x264.h and matching static library"
        );
        (
            prefix.join("include"),
            prefix.join("lib"),
            if windows { "libx264" } else { "x264" },
        )
    } else {
        assert!(
            !windows,
            "Windows builds require a static x264 prefix in MOLI_X264_DIR (see native/README.md)"
        );
        let source = out.join("x264");
        if !source.exists() {
            unpack("native/x264.tar.gz", &out);
        }
        let build = out.join("x264-build");
        fs::create_dir_all(&build).expect("x264 build directory");
        let compiler = cc::Build::new().get_compiler();
        let mut configure = Command::new("bash");
        configure
            .arg(source.join("configure"))
            .arg("--disable-cli")
            .arg("--enable-static")
            .arg("--enable-pic")
            .arg("--disable-opencl")
            .arg("--bit-depth=8")
            .arg("--chroma-format=420")
            .arg(format!("--host={target}"))
            .env("CC", compiler.path())
            .current_dir(&build);
        if matches!(target.split('-').next(), Some("x86_64" | "i686"))
            && env::var_os("AS").is_none()
        {
            configure.env("AS", x86_assembler(&out));
        }
        run(&mut configure);
        run(Command::new("make")
            .arg("-j")
            .arg(env::var("NUM_JOBS").unwrap_or_else(|_| "1".into()))
            .arg("libx264.a")
            .current_dir(&build));
        // Keep the generated configuration next to the exact public header.
        fs::copy(source.join("x264.h"), build.join("x264.h")).expect("copy x264 header");
        (build.clone(), build, "x264")
    };
    cc::Build::new()
        .file("native/bridge.c")
        .include(include)
        .opt_level(3)
        .compile("moli_x264_bridge");
    println!("cargo:rustc-link-search=native={}", library.display());
    println!("cargo:rustc-link-lib=static={library_name}");
    if !windows {
        println!("cargo:rustc-link-lib=m");
        println!("cargo:rustc-link-lib=pthread");
    }
}
