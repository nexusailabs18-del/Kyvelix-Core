#include <llvm/IR/Module.h>
#include <llvm/Passes/PassBuilder.h>
#include <llvm/Passes/StandardInstrumentations.h>
#include <llvm/Target/TargetMachine.h>
#include <llvm/IR/Verifier.h>
#include <llvm/Support/TargetSelect.h>
#include <llvm/Support/Host.h>
#include <llvm-c/Core.h>
#include <llvm-c/TargetMachine.h>
#include <cstring>
#include <string>

extern "C" {

// Run optimization pipeline on the given module.
// tier: 0 = O1, 1 = O2+vec, 2 = O3+vec+unroll
// Returns 0 on success, nonzero on failure.
int ky_run_optimization_pipeline(LLVMModuleRef moduleRef, int tier, char** errorOut) {
    if (!moduleRef) return 1;
    llvm::Module* M = llvm::unwrap(moduleRef);

    std::string triple = M->getTargetTriple();
    if (triple.empty()) triple = llvm::sys::getDefaultTargetTriple();
    std::string CPU = llvm::sys::getHostCPUName().str();

    // Detect host features
    llvm::StringMap<bool> hostFeatures;
    if (!llvm::sys::getHostCPUFeatures(hostFeatures)) {
        if (errorOut) *errorOut = strdup("Failed to detect host CPU features");
        return 2;
    }
    std::string features;
    for (auto &F : hostFeatures)
        features += (F.second ? "+" : "-") + F.first().str() + ",";

    std::string error;
    if (!target) {
        if (errorOut) *errorOut = strdup(error.c_str());
        return 3;
    }

    llvm::TargetOptions opt;
    auto RM = llvm::Reloc::Model::PIC_;
    auto TM = target->createTargetMachine(triple, CPU, features, opt, RM);
    if (!TM) {
        if (errorOut) *errorOut = strdup("Failed to create target machine");
        return 4;
    }
    M->setDataLayout(TM->createDataLayout());

    llvm::LoopAnalysisManager LAM;
    llvm::FunctionAnalysisManager FAM;
    llvm::CGSCCAnalysisManager CGAM;
    llvm::ModuleAnalysisManager MAM;

    llvm::PassBuilder PB(TM);
    PB.registerModuleAnalyses(MAM);
    PB.registerCGSCCAnalyses(CGAM);
    PB.registerFunctionAnalyses(FAM);
    PB.registerLoopAnalyses(LAM);
    PB.crossRegisterProxies(LAM, FAM, CGAM, MAM);

    llvm::ModulePassManager MPM;
    if (tier == 0) {
        MPM = PB.buildPerModuleDefaultPipeline(llvm::OptimizationLevel::O1);
    } else if (tier == 1) {
        MPM = PB.buildPerModuleDefaultPipeline(llvm::OptimizationLevel::O2);
        // Explicitly add vectorization passes (already in O2 but ensure)
    } else {
        MPM = PB.buildPerModuleDefaultPipeline(llvm::OptimizationLevel::O3);
    }

    MPM.run(*M, MAM);
    if (llvm::verifyModule(*M, &llvm::errs())) {
        if (errorOut) *errorOut = strdup("Module verification failed");
        return 5;
    }
    return 0;
}

} // extern "C"
