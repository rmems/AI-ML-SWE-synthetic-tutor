# gpu.jl — GPU Utilities
#
# Detect CUDA GPU availability and run simple kernel demos.
#
# Optional dependency: CUDA.jl
#   Install: using Pkg; Pkg.add("CUDA")
#   Then restart Julia and `using NeuroMathPlayground`.
#
# All functions fall back gracefully to CPU equivalents when no GPU is present.

const _CUDA_UUID = Base.UUID("052768ef-5323-5732-b1bb-66c8b64840ba")

"""
    gpu_available() -> Bool

Return `true` if CUDA.jl is loaded and a functional CUDA GPU is available.

Install CUDA.jl with: `using Pkg; Pkg.add("CUDA")`
Then load it before (or after) this module: `using CUDA`
"""
function gpu_available()::Bool
    cuda = get(Base.loaded_modules, Base.PkgId(_CUDA_UUID, "CUDA"), nothing)
    cuda === nothing && return false
    try
        return cuda.functional()::Bool
    catch
        return false
    end
end

"""
    gpu_info()

Print GPU and CUDA environment information.

If no CUDA GPU is detected, prints setup instructions instead.

# Example
```julia
using NeuroMathPlayground
gpu_info()
```
"""
function gpu_info()
    cuda = get(Base.loaded_modules, Base.PkgId(_CUDA_UUID, "CUDA"), nothing)
    if cuda === nothing
        println("✗  CUDA.jl is not loaded.")
        println("   Install with:  using Pkg; Pkg.add(\"CUDA\")")
        println("   Then run:      using CUDA")
        println()
        println("   CPU fallback is available for all operations.")
        return
    end

    try
        if !cuda.functional()
            println("✗  CUDA.jl is loaded but no functional GPU was found.")
            println("   Check your NVIDIA driver installation.")
            return
        end

        println("✓  CUDA is functional")
        cuda.versioninfo()
    catch e
        println("✗  CUDA.jl is loaded but an error occurred while querying the GPU.")
        println("   Error: ", sprint(showerror, e))
    end
end

"""
    cuda_kernel_demo()

Run a simple vector-add kernel as a GPU sanity check.

On a CUDA GPU the kernel runs on device memory.
Without a GPU, an equivalent CPU computation is shown instead.

# Example
```julia
using NeuroMathPlayground
cuda_kernel_demo()
```
"""
function cuda_kernel_demo()
    cuda = get(Base.loaded_modules, Base.PkgId(_CUDA_UUID, "CUDA"), nothing)

    n = 1_000_000

    use_gpu = false
    if cuda !== nothing
        try
            use_gpu = cuda.functional()
        catch
            use_gpu = false
        end
    end

    if use_gpu
        try
            # ── GPU path ──────────────────────────────────────────────────────────
            println("Running vector-add on GPU  (n = $n)")
            a = cuda.ones(Float32, n)
            b = cuda.fill(2f0, n)
            c = a .+ b
            s = sum(c)
            println("  result  = $s  (expected $(3f0 * n))")
            println("  device  = ", cuda.name(cuda.device()))
            return
        catch e
            println("CUDA kernel demo failed, falling back to CPU:")
            println("  ", sprint(showerror, e))
        end
    end

    # ── CPU fallback ──────────────────────────────────────────────────────────────
    println("No CUDA GPU available — running CPU equivalent  (n = $n)")
    a = ones(Float32, n)
    b = fill(2f0, n)
    c = a .+ b
    s = sum(c)
    println("  result  = $s  (expected $(3f0 * n))")
    println()
    println("  To enable GPU acceleration:")
    println("    using Pkg; Pkg.add(\"CUDA\")")
    println("    using CUDA")
    println("    cuda_kernel_demo()")
end
