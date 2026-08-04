import com.android.build.api.dsl.ApplicationExtension
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.kotlin.dsl.configure
import org.gradle.kotlin.dsl.get

const val TASK_GROUP = "rust"

open class Config {
    lateinit var rootDirRel: String
}

data class AndroidRustTarget(
    val abi: String,
    val arch: String,
    val target: String,
)

open class RustPlugin : Plugin<Project> {
    private lateinit var config: Config

    override fun apply(project: Project) = with(project) {
        config = extensions.create("rust", Config::class.java)

        val supportedTargets = listOf(
            AndroidRustTarget("arm64-v8a", "arm64", "aarch64"),
            AndroidRustTarget("armeabi-v7a", "arm", "armv7"),
        )

        fun requestedProperty(name: String) = (findProperty(name) as? String)
            ?.split(',')
            ?.map { it.trim() }
            ?.filter { it.isNotEmpty() }
            ?.toSet()

        val requestedAbis = requestedProperty("abiList")
        val requestedArchs = requestedProperty("archList")
        val requestedRustTargets = requestedProperty("targetList")
        val targets = supportedTargets.filter { target ->
            (requestedAbis == null || requestedAbis.contains(target.abi))
                && (requestedArchs == null || requestedArchs.contains(target.arch))
                && (requestedRustTargets == null || requestedRustTargets.contains(target.target))
        }
        if (targets.isEmpty()) {
            throw GradleException("Android builds support only armeabi-v7a and arm64-v8a ABI targets.")
        }
        val abiList = targets.map { it.abi }

        extensions.configure<ApplicationExtension> {
            @Suppress("UnstableApiUsage")
            flavorDimensions.add("abi")
            productFlavors {
                create("universal") {
                    dimension = "abi"
                    ndk {
                        abiFilters += abiList
                    }
                }
                targets.forEach { target ->
                    create(target.arch) {
                        dimension = "abi"
                        ndk {
                            abiFilters.add(target.abi)
                        }
                    }
                }
            }
        }

        afterEvaluate {
            for (profile in listOf("debug", "release")) {
                val profileCapitalized = profile.replaceFirstChar { it.uppercase() }
                val buildTask = tasks.maybeCreate(
                    "rustBuildUniversal$profileCapitalized",
                    DefaultTask::class.java
                ).apply {
                    group = TASK_GROUP
                    description = "Build dynamic library in $profile mode for all targets"
                }

                tasks["mergeUniversal${profileCapitalized}JniLibFolders"].dependsOn(buildTask)

                for (androidTarget in targets) {
                    val targetArchCapitalized = androidTarget.arch.replaceFirstChar { it.uppercase() }
                    val targetBuildTask = project.tasks.maybeCreate(
                        "rustBuild$targetArchCapitalized$profileCapitalized",
                        BuildTask::class.java
                    ).apply {
                        group = TASK_GROUP
                        description = "Build dynamic library in $profile mode for ${androidTarget.arch}"
                        rootDirRel = config.rootDirRel
                        target = androidTarget.target
                        release = profile == "release"
                    }

                    buildTask.dependsOn(targetBuildTask)
                    tasks["merge$targetArchCapitalized${profileCapitalized}JniLibFolders"].dependsOn(
                        targetBuildTask
                    )
                }
            }
        }
    }
}
