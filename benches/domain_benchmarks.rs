use criterion::{Criterion, black_box, criterion_group, criterion_main};
use sha2::{Digest, Sha256};
use testable_rust_architecture_template::domain::CreateItemRequest;
use validator::Validate;

fn bench_validation(c: &mut Criterion) {
    let request = CreateItemRequest::new(
        "Standard Item Name".to_string(),
        "This is some content that is being validated. It's a standard size content string."
            .to_string(),
    );

    c.bench_function("validate_create_item_request", |b| {
        b.iter(|| {
            let _ = black_box(&request).validate();
        })
    });
}

fn bench_hashing(c: &mut Criterion) {
    let data = "Some reasonably long content string that we want to hash using SHA256 to simulate the hashing done in the service layer.".repeat(10);

    c.bench_function("sha256_hashing", |b| {
        b.iter(|| {
            let mut hasher = Sha256::new();
            hasher.update(black_box(&data).as_bytes());
            let _ = hasher.finalize();
        })
    });
}

criterion_group!(benches, bench_validation, bench_hashing);
criterion_main!(benches);
