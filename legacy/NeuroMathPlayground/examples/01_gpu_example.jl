# examples/01_gpu_example.jl
#
# GPU detection and CUDA kernel demo.
#
# Run from the project root:
#   julia --project=. examples/01_gpu_example.jl
#
# To use your RTX 5080 GPU, first install CUDA.jl:
#   julia --project=. -e 'using Pkg; Pkg.add("CUDA")'
# Then either:
#   - uncomment the `using CUDA` line below, OR
#   - run `using CUDA` in the REPL before including this file.

using NeuroMathPlayground

# Uncomment to enable GPU support (requires CUDA.jl to be installed):
# using CUDA

println("=" ^ 60)
println("  GPU Example")
println("=" ^ 60)
println()

# ── Step 1: Detect GPU ────────────────────────────────────────────────────────
println("Step 1: GPU availability check")
println("  gpu_available() = $(gpu_available())")
println()

# ── Step 2: Print GPU info ────────────────────────────────────────────────────
println("Step 2: GPU information")
gpu_info()
println()

# ── Step 3: Vector-add kernel ─────────────────────────────────────────────────
println("Step 3: Vector-add kernel demo")
cuda_kernel_demo()
println()

# ── Step 4: What you can do on the RTX 5080 ──────────────────────────────────
println("Step 4: GPU-accelerated neuromorphic computing tips")
println("""
  Once CUDA.jl is installed and your GPU is available:

  using CUDA

  # Allocate GPU arrays
  n = 1_000_000
  v_gpu  = CUDA.zeros(Float32, n)   # neuron voltages on GPU
  I_gpu  = CUDA.ones(Float32, n)    # input currents

  # Broadcast standard Julia operations over GPU arrays
  v_gpu .+= 0.01f0 .* (I_gpu .- v_gpu)   # Euler step on GPU

  # Transfer result back to CPU
  v_cpu = Array(v_gpu)

  # For custom CUDA kernels use @cuda:
  function add_kernel!(c, a, b)
      i = (blockIdx().x - 1) * blockDim().x + threadIdx().x
      i <= length(c) && (c[i] = a[i] + b[i])
      return
  end

  a, b, c = CUDA.ones(Float32, n), CUDA.ones(Float32, n), CUDA.zeros(Float32, n)
  @cuda threads=256 blocks=cld(n, 256) add_kernel!(c, a, b)
  CUDA.synchronize()
""")
