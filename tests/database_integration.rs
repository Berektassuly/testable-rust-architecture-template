//! Database integration tests using testcontainers.
//!
//! These tests require Docker to be running and use testcontainers
//! to spin up a real PostgreSQL instance.

use testcontainers::{GenericImage, ImageExt, runners::AsyncRunner};

use std::collections::HashMap;
use testable_rust_architecture_template::domain::{
    BlockchainStatus, CreateItemRequest, DatabaseClient, ItemMetadataRequest,
};
use testable_rust_architecture_template::infra::{PostgresClient, PostgresConfig};

/// Helper to create a PostgreSQL container and client
async fn setup_postgres() -> (PostgresClient, testcontainers::ContainerAsync<GenericImage>) {
    let container = GenericImage::new("postgres", "16-alpine")
        .with_env_var("POSTGRES_USER", "test")
        .with_env_var("POSTGRES_PASSWORD", "test")
        .with_env_var("POSTGRES_DB", "test_db")
        .with_exposed_port(5432.into())
        .start()
        .await
        .expect("Failed to start postgres container");

    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("Failed to get postgres port");

    let database_url = format!("postgres://test:test@127.0.0.1:{}/test_db", port);

    // Wait for postgres to be ready
    let mut attempts = 0;
    let client = loop {
        attempts += 1;
        match PostgresClient::new(&database_url, PostgresConfig::default()).await {
            Ok(client) => break client,
            Err(_) if attempts < 30 => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Err(e) => panic!("Failed to connect to postgres after 30 attempts: {:?}", e),
        }
    };

    // Run migrations
    client
        .run_migrations()
        .await
        .expect("Failed to run migrations");

    (client, container)
}

#[tokio::test]
async fn test_create_and_get_item() {
    let (client, _container) = setup_postgres().await;

    let request = CreateItemRequest::new("Test Item".to_string(), "Test content".to_string());

    // Create item
    let created = client
        .create_item(&request)
        .await
        .expect("Failed to create item");
    assert_eq!(created.name, "Test Item");
    assert_eq!(created.content, "Test content");
    assert!(created.id.starts_with("item_"));

    // Get item
    let fetched = client
        .get_item(&created.id)
        .await
        .expect("Failed to get item")
        .expect("Item not found");

    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.name, created.name);
    assert_eq!(fetched.content, created.content);
}

#[tokio::test]
async fn test_create_item_with_metadata() {
    let (client, _container) = setup_postgres().await;

    let mut custom_fields = HashMap::new();
    custom_fields.insert("key1".to_string(), "value1".to_string());

    let request = CreateItemRequest {
        name: "Item with Metadata".to_string(),
        description: Some("A description".to_string()),
        content: "Content here".to_string(),
        metadata: Some(ItemMetadataRequest {
            author: Some("John Doe".to_string()),
            version: Some("1.0.0".to_string()),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
            custom_fields,
        }),
    };

    let created = client
        .create_item(&request)
        .await
        .expect("Failed to create item");
    assert_eq!(created.description, Some("A description".to_string()));

    let metadata = created.metadata.expect("Metadata should be present");
    assert_eq!(metadata.author, Some("John Doe".to_string()));
    assert_eq!(metadata.version, Some("1.0.0".to_string()));
    assert_eq!(metadata.tags, vec!["tag1".to_string(), "tag2".to_string()]);
}

#[tokio::test]
async fn test_list_items_pagination() {
    let (client, _container) = setup_postgres().await;

    // Create 5 items
    for i in 0..5 {
        let request = CreateItemRequest::new(format!("Item {}", i), format!("Content {}", i));
        client
            .create_item(&request)
            .await
            .expect("Failed to create item");
        // Small delay to ensure different timestamps
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // Get first page (limit 2)
    let page1 = client
        .list_items(2, None)
        .await
        .expect("Failed to list items");
    assert_eq!(page1.items.len(), 2);
    assert!(page1.has_more);
    assert!(page1.next_cursor.is_some());

    // Get second page
    let page2 = client
        .list_items(2, page1.next_cursor.as_deref())
        .await
        .expect("Failed to list items");
    assert_eq!(page2.items.len(), 2);
    assert!(page2.has_more);

    // Get third page
    let page3 = client
        .list_items(2, page2.next_cursor.as_deref())
        .await
        .expect("Failed to list items");
    assert_eq!(page3.items.len(), 1);
    assert!(!page3.has_more);
    assert!(page3.next_cursor.is_none());

    // Verify no duplicates across pages
    let all_ids: Vec<&str> = page1
        .items
        .iter()
        .chain(page2.items.iter())
        .chain(page3.items.iter())
        .map(|i| i.id.as_str())
        .collect();
    let unique_ids: std::collections::HashSet<&str> = all_ids.iter().copied().collect();
    assert_eq!(all_ids.len(), unique_ids.len());
}

#[tokio::test]
async fn test_blockchain_status_updates() {
    let (client, _container) = setup_postgres().await;

    let request = CreateItemRequest::new("Test Item".to_string(), "Content".to_string());
    let created = client
        .create_item(&request)
        .await
        .expect("Failed to create item");
    assert_eq!(created.blockchain_status, BlockchainStatus::Pending);

    // Update to pending submission
    client
        .update_blockchain_status(
            &created.id,
            BlockchainStatus::PendingSubmission,
            None,
            Some("Initial error"),
            Some(chrono::Utc::now()),
        )
        .await
        .expect("Failed to update status");

    let fetched = client
        .get_item(&created.id)
        .await
        .expect("Failed to get item")
        .expect("Item not found");
    assert_eq!(
        fetched.blockchain_status,
        BlockchainStatus::PendingSubmission
    );
    assert_eq!(
        fetched.blockchain_last_error,
        Some("Initial error".to_string())
    );

    // Update to submitted
    client
        .update_blockchain_status(
            &created.id,
            BlockchainStatus::Submitted,
            Some("signature123"),
            None,
            None,
        )
        .await
        .expect("Failed to update status");

    let fetched = client
        .get_item(&created.id)
        .await
        .expect("Failed to get item")
        .expect("Item not found");
    assert_eq!(fetched.blockchain_status, BlockchainStatus::Submitted);
    assert_eq!(
        fetched.blockchain_signature,
        Some("signature123".to_string())
    );
}

#[tokio::test]
async fn test_get_pending_blockchain_items() {
    let (client, _container) = setup_postgres().await;

    // Create items with different statuses
    for i in 0..3 {
        let request = CreateItemRequest::new(format!("Item {}", i), "Content".to_string());
        let item = client
            .create_item(&request)
            .await
            .expect("Failed to create item");

        if i == 0 {
            // Leave as pending
        } else if i == 1 {
            // Set to pending_submission
            client
                .update_blockchain_status(
                    &item.id,
                    BlockchainStatus::PendingSubmission,
                    None,
                    None,
                    None,
                )
                .await
                .expect("Failed to update status");
        } else {
            // Set to confirmed
            client
                .update_blockchain_status(
                    &item.id,
                    BlockchainStatus::Confirmed,
                    Some("sig"),
                    None,
                    None,
                )
                .await
                .expect("Failed to update status");
        }
    }

    let pending = client
        .get_pending_blockchain_items(10)
        .await
        .expect("Failed to get pending items");

    // Only the item with pending_submission status should be returned
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].blockchain_status,
        BlockchainStatus::PendingSubmission
    );
}

#[tokio::test]
async fn test_increment_retry_count() {
    let (client, _container) = setup_postgres().await;

    let request = CreateItemRequest::new("Test Item".to_string(), "Content".to_string());
    let created = client
        .create_item(&request)
        .await
        .expect("Failed to create item");
    assert_eq!(created.blockchain_retry_count, 0);

    // Increment retry count
    let count1 = client
        .increment_retry_count(&created.id)
        .await
        .expect("Failed to increment");
    assert_eq!(count1, 1);

    let count2 = client
        .increment_retry_count(&created.id)
        .await
        .expect("Failed to increment");
    assert_eq!(count2, 2);

    // Verify in database
    let fetched = client
        .get_item(&created.id)
        .await
        .expect("Failed to get item")
        .expect("Item not found");
    assert_eq!(fetched.blockchain_retry_count, 2);
}

#[tokio::test]
async fn test_health_check() {
    let (client, _container) = setup_postgres().await;

    let result = client.health_check().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_nonexistent_item() {
    let (client, _container) = setup_postgres().await;

    let result = client
        .get_item("nonexistent_id")
        .await
        .expect("Query should succeed");
    assert!(result.is_none());
}
