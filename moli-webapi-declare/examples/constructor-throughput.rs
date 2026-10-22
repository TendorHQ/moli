//! Compare identical native callbacks with and without deferred construction.
//! Use the browser's pointer-compressed V8 configuration:
//! cargo run --release -p moli-webapi-declare --features v8/v8_enable_pointer_compression --example constructor-throughput -- 100000 9
//! Arguments select iterations per sample and sample count. CSV reports medians;
//! compilation, warmup, and explicit GC are outside the timed sections. This
//! isolates binding overhead, rather than measuring DOMParser or WebRTC work.

use std::{pin::pin, time::Instant};

use moli_v8_test_util::ensure_v8;
use moli_v8_util::v8str;
use moli_webapi_declare::{
    capture_web_api_constructor_intrinsics, initialize_web_api_constructor_receiver,
    web_api_constructor_with_deferred_prototype,
};

moli_webapi_declare::declare_web_api_interfaces! {
    BenchmarkObject;
}

fn constructor<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s>,
) {
    let Some(number) = args.get(0).to_number(scope) else {
        return;
    };
    if !initialize_web_api_constructor_receiver(scope, args.this(), "BenchmarkObject") {
        return;
    }
    if args
        .this()
        .set(scope, v8str(scope, "value").into(), number.into())
        .unwrap_or(false)
    {
        rv.set(args.this().into());
    }
}

fn eval<'s>(scope: &mut v8::PinScope<'s, '_>, source: &str) -> v8::Local<'s, v8::Value> {
    let source = v8::String::new(scope, source).unwrap();
    v8::Script::compile(scope, source, None)
        .unwrap()
        .run(scope)
        .unwrap()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations: u32 = args
        .next()
        .map(|value| value.parse().expect("iterations must be an integer"))
        .unwrap_or(100_000);
    let samples: usize = args
        .next()
        .map(|value| value.parse().expect("samples must be an integer"))
        .unwrap_or(9);
    assert!((1..=1_000_000).contains(&iterations));
    assert!((1..=100).contains(&samples));
    assert!(args.next().is_none(), "expected [iterations] [samples]");

    ensure_v8();
    let mut isolate = v8::Isolate::new(v8::CreateParams::default());
    let scope = pin!(v8::HandleScope::new(&mut isolate));
    let scope = &mut scope.init();
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    capture_web_api_constructor_intrinsics(scope).unwrap();
    let template = v8::FunctionTemplate::builder(moli_webapi_declare::web_api_constructor!(
        BenchmarkObject,
        constructor
    ))
    .build(scope);
    template.set_class_name(v8str(scope, "BenchmarkObject"));
    let native = template.get_function(scope).unwrap();
    let deferred = web_api_constructor_with_deferred_prototype(scope, native).unwrap();
    let global = context.global(scope);
    global
        .set(scope, v8str(scope, "Native").into(), native.into())
        .unwrap();
    global
        .set(scope, v8str(scope, "Deferred").into(), deferred.into())
        .unwrap();
    let cases = eval(
        scope,
        "[Native, Deferred, class extends Native {}, class extends Deferred {}]",
    );
    let cases = v8::Local::<v8::Array>::try_from(cases).unwrap();
    let runner = eval(
        scope,
        "(function(C, n) { let sum = 0; for (let i = 0; i < n; ++i) sum += new C(i).value; return sum; })",
    );
    let runner = v8::Local::<v8::Function>::try_from(runner).unwrap();
    let mut timings = [const { Vec::new() }; 4];
    let receiver = v8::undefined(scope).into();
    let warmup = v8::Integer::new(scope, 10_000).into();
    for index in 0..cases.length() {
        let case = cases.get_index(scope, index).unwrap();
        runner.call(scope, receiver, &[case, warmup]).unwrap();
    }
    let count = v8::Integer::new_from_unsigned(scope, iterations).into();
    let expected = f64::from(iterations) * f64::from(iterations - 1) / 2.0;
    for sample in 0..samples {
        // Alternate order so neither path always receives a warmer heap.
        for offset in 0..4 {
            let index = (sample + offset) % 4;
            let case = cases.get_index(scope, index as u32).unwrap();
            scope.low_memory_notification();
            let start = Instant::now();
            let result = runner.call(scope, receiver, &[case, count]).unwrap();
            let elapsed = start.elapsed().as_secs_f64();
            assert_eq!(result.number_value(scope).unwrap(), expected);
            timings[index].push(elapsed * 1e9 / f64::from(iterations));
        }
    }
    println!("case,iterations,samples,median_ns_per_construct,constructs_per_second");
    for (name, values) in ["native", "deferred", "native_subclass", "deferred_subclass"]
        .into_iter()
        .zip(&mut timings)
    {
        values.sort_by(f64::total_cmp);
        let median = values[values.len() / 2];
        println!(
            "{name},{iterations},{samples},{median:.2},{:.0}",
            1e9 / median
        );
    }
}
