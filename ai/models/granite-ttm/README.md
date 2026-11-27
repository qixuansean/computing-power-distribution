# IBM Granite TinyTimeMixer (TTM) for OCN

## Overview
- **Model Family**: IBM Granite TinyTimeMixer (TTM-R2)
- **Hugging Face ID**: `ibm-granite/granite-timeseries-ttm-r2`
- **Paper**: Tiny Time Mixers (TTMs): Fast Pre-trained Models for Enhanced Zero/Few-Shot Forecasting of Multivariate Time Series (NeurIPS 2024)
- **License**: Apache 2.0 (Commercial & Enterprise Friendly)
- **Parameters**: ~805k (< 1M parameters)
- **Format**: Safetensors / PyTorch

## Usage in OCN
Used for multivariate time-series forecasting of node resources (CPU usage, RAM usage, network bandwidth trends) to improve scheduling decisions.

## Limitations & Constraints
- Requires input data to be standard-scaled.
- Optimized for CPU/GPU inference in resource-constrained environments.
