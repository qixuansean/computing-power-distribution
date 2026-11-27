use crate::task::{ResourceRequirements, TaskValidationError};
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceStrategy {
    pub min_battery_percent: u32,
    pub require_charging: bool,
    pub max_temp_celsius: f32,
    pub cpu_usage_limit: f32,
    pub ram_usage_limit: f32,
}

impl Default for ResourceStrategy {
    fn default() -> Self {
        Self {
            min_battery_percent: 20,
            require_charging: false,
            max_temp_celsius: 65.0,
            cpu_usage_limit: 0.8,
            ram_usage_limit: 0.8,
        }
    }
}

impl ResourceStrategy {
    /// Validate strategy parameters
    pub fn validate(&self) -> Result<(), TaskValidationError> {
        if self.min_battery_percent > 100 {
            return Err(TaskValidationError::InvalidResourceRequirements);
        }

        if !self.max_temp_celsius.is_finite() || self.max_temp_celsius < 0.0 || self.max_temp_celsius > 120.0 {
            return Err(TaskValidationError::InvalidResourceRequirements);
        }

        if !self.cpu_usage_limit.is_finite() || self.cpu_usage_limit <= 0.0 || self.cpu_usage_limit > 1.0 {
            return Err(TaskValidationError::InvalidResourceRequirements);
        }

        if !self.ram_usage_limit.is_finite() || self.ram_usage_limit <= 0.0 || self.ram_usage_limit > 1.0 {
            return Err(TaskValidationError::InvalidResourceRequirements);
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct SystemMetrics {
    pub cpu_usage: f32,          // 0.0 to 1.0
    pub ram_usage: f32,          // 0.0 to 1.0
    pub battery_percent: u32,    // 0 to 100
    pub is_charging: bool,
    pub temperature_celsius: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("Low battery: {0}% (minimum: {1}%)")]
    LowBattery(u32, u32),
    #[error("Device not charging (charging required)")]
    NotCharging,
    #[error("High CPU usage: {0:.1}% (limit: {1:.1}%)")]
    HighCpuUsage(f32, f32),
    #[error("High RAM usage: {0:.1}% (limit: {1:.1}%)")]
    HighRamUsage(f32, f32),
    #[error("High temperature: {0:.1}°C (limit: {1:.1}°C)")]
    HighTemperature(f32, f32),
    #[error("Invalid metrics: {0}")]
    InvalidMetrics(String),
}

pub struct ResourceManager {
    pub strategy: ResourceStrategy,
}

impl ResourceManager {
    pub fn new(strategy: ResourceStrategy) -> Result<Self, TaskValidationError> {
        strategy.validate()?;
        Ok(Self { strategy })
    }

    /// Check if the system can accept a new task based on current metrics
    pub fn can_accept_task(&self, metrics: &SystemMetrics) -> Result<(), ResourceError> {
        // Validate metrics are finite and in range
        if !metrics.cpu_usage.is_finite() || metrics.cpu_usage < 0.0 || metrics.cpu_usage > 1.0 {
            return Err(ResourceError::InvalidMetrics("CPU usage out of range [0.0, 1.0]".into()));
        }

        if !metrics.ram_usage.is_finite() || metrics.ram_usage < 0.0 || metrics.ram_usage > 1.0 {
            return Err(ResourceError::InvalidMetrics("RAM usage out of range [0.0, 1.0]".into()));
        }

        if !metrics.temperature_celsius.is_finite() || metrics.temperature_celsius < -50.0 || metrics.temperature_celsius > 150.0 {
            return Err(ResourceError::InvalidMetrics("Temperature out of reasonable range".into()));
        }

        if metrics.battery_percent > 100 {
            return Err(ResourceError::InvalidMetrics("Battery percent out of range [0, 100]".into()));
        }

        // Check battery
        if metrics.battery_percent < self.strategy.min_battery_percent {
            return Err(ResourceError::LowBattery(
                metrics.battery_percent,
                self.strategy.min_battery_percent
            ));
        }

        // Check charging requirement
        if self.strategy.require_charging && !metrics.is_charging {
            return Err(ResourceError::NotCharging);
        }

        // Check CPU usage
        let cpu_percent = metrics.cpu_usage * 100.0;
        let cpu_limit = self.strategy.cpu_usage_limit * 100.0;
        if cpu_percent > cpu_limit {
            return Err(ResourceError::HighCpuUsage(cpu_percent, cpu_limit));
        }

        // Check RAM usage
        let ram_percent = metrics.ram_usage * 100.0;
        let ram_limit = self.strategy.ram_usage_limit * 100.0;
        if ram_percent > ram_limit {
            return Err(ResourceError::HighRamUsage(ram_percent, ram_limit));
        }

        // Check temperature
        if metrics.temperature_celsius > self.strategy.max_temp_celsius {
            return Err(ResourceError::HighTemperature(
                metrics.temperature_celsius,
                self.strategy.max_temp_celsius
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_strategy() -> ResourceStrategy {
        ResourceStrategy::default()
    }

    fn default_metrics() -> SystemMetrics {
        SystemMetrics {
            cpu_usage: 0.3,
            ram_usage: 0.4,
            battery_percent: 80,
            is_charging: true,
            temperature_celsius: 35.0,
        }
    }

    #[test]
    fn test_default_strategy_validation() {
        let strategy = default_strategy();
        assert!(strategy.validate().is_ok());
    }

    #[test]
    fn test_can_accept_task_success() {
        let manager = ResourceManager::new(default_strategy()).unwrap();
        let metrics = default_metrics();
        assert!(manager.can_accept_task(&metrics).is_ok());
    }

    #[test]
    fn test_low_battery_rejection() {
        let manager = ResourceManager::new(default_strategy()).unwrap();
        let mut metrics = default_metrics();
        metrics.battery_percent = 10; // Below default 20%
        assert!(matches!(manager.can_accept_task(&metrics), Err(ResourceError::LowBattery(10, 20))));
    }

    #[test]
    fn test_charging_required_rejection() {
        let strategy = ResourceStrategy {
            require_charging: true,
            ..default_strategy()
        };
        let manager = ResourceManager::new(strategy).unwrap();
        let mut metrics = default_metrics();
        metrics.is_charging = false;
        assert!(matches!(manager.can_accept_task(&metrics), Err(ResourceError::NotCharging)));
    }

    #[test]
    fn test_high_cpu_rejection() {
        let manager = ResourceManager::new(default_strategy()).unwrap();
        let mut metrics = default_metrics();
        metrics.cpu_usage = 0.9; // Above default 0.8
        assert!(matches!(manager.can_accept_task(&metrics), Err(ResourceError::HighCpuUsage(_, _))));
    }

    #[test]
    fn test_high_ram_rejection() {
        let manager = ResourceManager::new(default_strategy()).unwrap();
        let mut metrics = default_metrics();
        metrics.ram_usage = 0.9; // Above default 0.8
        assert!(matches!(manager.can_accept_task(&metrics), Err(ResourceError::HighRamUsage(_, _))));
    }

    #[test]
    fn test_high_temperature_rejection() {
        let manager = ResourceManager::new(default_strategy()).unwrap();
        let mut metrics = default_metrics();
        metrics.temperature_celsius = 70.0; // Above default 65.0
        assert!(matches!(manager.can_accept_task(&metrics), Err(ResourceError::HighTemperature(_, _))));
    }

    #[test]
    fn test_nan_cpu_rejection() {
        let manager = ResourceManager::new(default_strategy()).unwrap();
        let mut metrics = default_metrics();
        metrics.cpu_usage = f32::NAN;
        assert!(matches!(manager.can_accept_task(&metrics), Err(ResourceError::InvalidMetrics(_))));
    }

    #[test]
    fn test_nan_ram_rejection() {
        let manager = ResourceManager::new(default_strategy()).unwrap();
        let mut metrics = default_metrics();
        metrics.ram_usage = f32::NAN;
        assert!(matches!(manager.can_accept_task(&metrics), Err(ResourceError::InvalidMetrics(_))));
    }

    #[test]
    fn test_ram_out_of_range() {
        let manager = ResourceManager::new(default_strategy()).unwrap();
        let mut metrics = default_metrics();
        metrics.ram_usage = 1.5; // > 1.0
        assert!(matches!(manager.can_accept_task(&metrics), Err(ResourceError::InvalidMetrics(_))));
    }

    #[test]
    fn test_battery_out_of_range() {
        let manager = ResourceManager::new(default_strategy()).unwrap();
        let mut metrics = default_metrics();
        metrics.battery_percent = 150;
        assert!(matches!(manager.can_accept_task(&metrics), Err(ResourceError::InvalidMetrics(_))));
    }

    #[test]
    fn test_strategy_validation_invalid_cpu_limit() {
        let mut strategy = default_strategy();
        strategy.cpu_usage_limit = 1.5; // > 1.0
        assert!(strategy.validate().is_err());
    }

    #[test]
    fn test_strategy_validation_nan_temp() {
        let mut strategy = default_strategy();
        strategy.max_temp_celsius = f32::NAN;
        assert!(strategy.validate().is_err());
    }
}