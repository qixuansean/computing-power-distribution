# OCN AI Directory Overview

Welcome to the **OCN AI Module (`ai/`)**, the specialized artificial intelligence layer for the OpenCompute Network (OCN).

## Objectives

1. **Resource Prediction**: Utilizing compact, state-of-the-art pre-trained time-series models (**IBM Granite TinyTimeMixer**) for multivariate CPU, RAM, and network traffic forecasting.
2. **Strict Safety Boundary**: AI only provides recommendations. Deterministic schedulers and policy engines validate every decision.
3. **Decoupled Architecture**: Model weights, adapters, and runtime managers are completely separated from OCN Core.

## Directory Structure

```text
ai/
├── models/
│   ├── needle2/
│   │   └── README.md
│   └── granite-ttm/
│       ├── model/
│       ├── config/
│       └── README.md
├── adapters/
│   ├── needle2_adapter/
│   └── granite_ttm_adapter/
├── runtime/
│   ├── cpu/
│   └── android/
├── configs/
├── benchmarks/
├── scripts/
├── manifests/
└── README.md
```
