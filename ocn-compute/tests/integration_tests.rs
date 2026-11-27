use ocn_compute::task::{TaskDescriptor, ResourceRequirements, TaskType, TaskStatus, MAX_PAYLOAD_SIZE, ExecutionResult};
use ocn_compute::runtime::ComputeExecutor;
use ocn_compute::resource_manager::{ResourceManager, ResourceStrategy, SystemMetrics};
use ocn_compute::deduplication::{TaskDeduplicationCache, CacheCheckResult};
use sha2::{Sha256, Digest};
use std::time::{SystemTime, UNIX_EPOCH, Duration};

fn make_task(payload: &[u8], idem: &str) -> TaskDescriptor {
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let input_hash: [u8; 32] = hasher.finalize().into();

    TaskDescriptor {
        task_id: format!("task-{}", idem),
        task_type: TaskType::BatchHash,
        protocol_version: 1,
        input_hash,
        payload: payload.to_vec(),
        resource_requirements: ResourceRequirements {
            cpu_cores: 1,
            memory_mb: 64,
            gpu_required: false,
        },
        priority: 50,
        deadline_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64 + 60000,
        timeout_ms: 5000,
        idempotency_key: idem.to_string(),
    }
}

#[test]
fn test_integration_normal_batch_hash() {
    let payload = b"integration test batch hash payload";
    let task = make_task(payload, "idem-int-1");
    assert!(task.validate().is_ok());

    let result = ComputeExecutor::execute(&task).unwrap();
    assert_eq!(result.status, TaskStatus::Success);
    assert!(result.validate().is_ok());
}

#[test]
fn test_integration_error_input_hash() {
    let mut task = make_task(b"payload", "idem-int-2");
    task.input_hash = [0xFF; 32]; // Wrong hash
    assert!(task.validate().is_err());
}

#[test]
fn test_integration_error_result_hash() {
    let result = b"some result";
    let exec_res = ExecutionResult {
        task_id: "t1".to_string(),
        node_id: "n1".to_string(),
        result: result.to_vec(),
        result_hash: [0xAA; 32], // Wrong hash
        execution_time_ms: 10,
        status: TaskStatus::Success,
        error_code: None,
    };
    assert!(exec_res.validate().is_err());
}

#[test]
fn test_integration_oversized_payload() {
    let mut task = make_task(b"small", "idem-int-3");
    task.payload = vec![0u8; MAX_PAYLOAD_SIZE + 1];
    let mut hasher = Sha256::new();
    hasher.update(&task.payload);
    task.input_hash = hasher.finalize().into();

    assert!(task.validate().is_err());
}

#[test]
fn test_integration_invalid_priority() {
    let mut task = make_task(b"payload", "idem-int-4");
    task.priority = 150; // > 100
    assert!(task.validate().is_err());
}

#[test]
fn test_integration_invalid_timeout() {
    let mut task = make_task(b"payload", "idem-int-5");
    task.timeout_ms = 100; // < 1000
    assert!(task.validate().is_err());
}

#[test]
fn test_integration_resource_manager_checks() {
    let manager = ResourceManager::new(ResourceStrategy::default()).unwrap();

    // Normal metrics -> OK
    let normal_metrics = SystemMetrics {
        cpu_usage: 0.2,
        ram_usage: 0.3,
        battery_percent: 90,
        is_charging: true,
        temperature_celsius: 32.0,
    };
    assert!(manager.can_accept_task(&normal_metrics).is_ok());

    // NaN CPU -> Error
    let nan_cpu = SystemMetrics {
        cpu_usage: f32::NAN,
        ..normal_metrics
    };
    assert!(manager.can_accept_task(&nan_cpu).is_err());

    // RAM out of range -> Error
    let bad_ram = SystemMetrics {
        ram_usage: 1.2,
        ..normal_metrics
    };
    assert!(manager.can_accept_task(&bad_ram).is_err());

    // Low battery -> Error
    let low_bat = SystemMetrics {
        battery_percent: 10,
        ..normal_metrics
    };
    assert!(manager.can_accept_task(&low_bat).is_err());

    // High temperature -> Error
    let high_temp = SystemMetrics {
        temperature_celsius: 85.0,
        ..normal_metrics
    };
    assert!(manager.can_accept_task(&high_temp).is_err());
}

#[tokio::test]
async fn test_integration_deduplication_flow() {
    let cache = TaskDeduplicationCache::new(Duration::from_secs(5), 50);
    let task = make_task(b"dedup payload", "idem-int-6");

    // First submit -> NewTask
    let check1 = cache.check_or_register(&task).await.unwrap();
    assert_eq!(check1, CacheCheckResult::NewTask);

    // Second submit while running -> AlreadyRunning
    let check2 = cache.check_or_register(&task).await.unwrap();
    assert_eq!(check2, CacheCheckResult::AlreadyRunning);

    // Execute task
    let exec_result = ComputeExecutor::execute(&task).unwrap();
    cache.store_completed(&task.idempotency_key, exec_result.clone()).await;

    // Third submit after completion -> Completed with result
    let check3 = cache.check_or_register(&task).await.unwrap();
    match check3 {
        CacheCheckResult::Completed(res) => {
            assert_eq!(res.task_id, task.task_id);
            assert_eq!(res.result_hash, exec_result.result_hash);
        }
        _ => panic!("Expected Completed result"),
    }
}
