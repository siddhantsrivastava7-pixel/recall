#!/usr/bin/env python3
"""
Generates Recall.xcodeproj/project.pbxproj for the Recall iOS app.
Run: python3 generate_xcodeproj.py
"""

import os, uuid, itertools

BASE = os.path.dirname(os.path.abspath(__file__))

# ── UUID factory ──────────────────────────────────────────────────────────────
_counter = itertools.count(1)
def make_id():
    n = next(_counter)
    return f"{n:024X}"

# ── File table ────────────────────────────────────────────────────────────────
# (display_name, relative_path, targets)
#   targets: list of "app" | "ext"
FILES = [
    # App sources
    ("RecallApp.swift",          "Recall/RecallApp.swift",                               ["app"]),
    ("RecallTheme.swift",        "Recall/DesignSystem/RecallTheme.swift",                ["app","ext"]),
    ("SharedConstants.swift",    "Recall/Shared/SharedConstants.swift",                  ["app","ext"]),
    ("Memory.swift",             "Recall/Models/Memory.swift",                           ["app","ext"]),
    ("PairingConfig.swift",      "Recall/Models/PairingConfig.swift",                    ["app","ext"]),
    ("MemoryStore.swift",        "Recall/Persistence/MemoryStore.swift",                 ["app","ext"]),
    ("KeychainService.swift",    "Recall/Services/KeychainService.swift",                ["app","ext"]),
    ("PairingService.swift",     "Recall/Services/PairingService.swift",                 ["app","ext"]),
    ("SyncService.swift",        "Recall/Services/SyncService.swift",                    ["app","ext"]),
    ("HomeViewModel.swift",      "Recall/ViewModels/HomeViewModel.swift",                ["app"]),
    ("SettingsViewModel.swift",  "Recall/ViewModels/SettingsViewModel.swift",            ["app"]),
    ("ContentView.swift",        "Recall/Views/ContentView.swift",                       ["app"]),
    ("HomeView.swift",           "Recall/Views/HomeView.swift",                          ["app"]),
    ("LibraryView.swift",        "Recall/Views/LibraryView.swift",                       ["app"]),
    ("SearchBarView.swift",      "Recall/Views/SearchBarView.swift",                     ["app"]),
    ("MemoryRowView.swift",      "Recall/Views/MemoryRowView.swift",                     ["app"]),
    ("MemoryDetailView.swift",   "Recall/Views/MemoryDetailView.swift",                  ["app"]),
    ("BottomNavView.swift",      "Recall/Views/BottomNavView.swift",                     ["app"]),
    ("SettingsView.swift",       "Recall/Views/SettingsView.swift",                      ["app"]),
    ("PairingView.swift",        "Recall/Views/PairingView.swift",                       ["app"]),
    ("QRScannerView.swift",      "Recall/Views/QRScannerView.swift",                     ["app"]),
    # Extension sources
    ("ShareViewController.swift","RecallShareExtension/ShareViewController.swift",       ["ext"]),
    ("ShareViewModel.swift",     "RecallShareExtension/ShareViewModel.swift",            ["ext"]),
    ("ShareView.swift",          "RecallShareExtension/ShareView.swift",                 ["ext"]),
]

# Info.plist files are NOT added to Resources build phase (processed via INFOPLIST_FILE setting).
# They still need PBXFileReferences for Xcode to display them in the navigator.
RESOURCE_FILES = []
INFOPLIST_REFS = [
    ("Info.plist",               "Recall/Resources/Info.plist"),
    ("ShareExtInfo.plist",       "RecallShareExtension/Info.plist"),
]
INFOPLIST_FILE_DATA = {}
for name, path in INFOPLIST_REFS:
    fref = make_id()
    INFOPLIST_FILE_DATA[path] = {"name": name, "ref": fref, "path": path}

# ── Assign UUIDs ──────────────────────────────────────────────────────────────
PROJECT_ID     = make_id()
MAIN_GROUP_ID  = make_id()
PRODUCTS_ID    = make_id()

APP_TARGET_ID        = make_id()
EXT_TARGET_ID        = make_id()
APP_BUILD_CONFIG_LIST = make_id()
EXT_BUILD_CONFIG_LIST = make_id()
PROJ_BUILD_CONFIG_LIST = make_id()

APP_DEBUG_CFG  = make_id()
APP_RELEASE_CFG = make_id()
EXT_DEBUG_CFG  = make_id()
EXT_RELEASE_CFG = make_id()
PROJ_DEBUG_CFG  = make_id()
PROJ_RELEASE_CFG = make_id()

APP_SOURCES_PHASE = make_id()
APP_RESOURCES_PHASE = make_id()
APP_EMBED_EXT_PHASE = make_id()
EXT_SOURCES_PHASE = make_id()
EXT_RESOURCES_PHASE = make_id()

APP_PRODUCT_REF = make_id()
EXT_PRODUCT_REF = make_id()
EXT_EMBED_BF    = make_id()

SCHEME_ID = make_id()

# Per-file UUIDs
file_data = {}
for name, path, targets in FILES + RESOURCE_FILES:
    fref = make_id()
    build_ids = {t: make_id() for t in targets}
    file_data[path] = {"name": name, "ref": fref, "builds": build_ids, "targets": targets, "path": path}

# Groups
RECALL_GROUP    = make_id()
EXT_GROUP       = make_id()
DESIGN_GROUP    = make_id()
SHARED_GROUP    = make_id()
MODELS_GROUP    = make_id()
PERSIST_GROUP   = make_id()
SERVICES_GROUP  = make_id()
VIEWMODELS_GROUP = make_id()
VIEWS_GROUP     = make_id()
RESOURCES_GROUP = make_id()

def src_refs(target):
    return [d for d in file_data.values() if target in d["targets"] and d["path"].endswith(".swift")]

def res_refs(target):
    return [d for d in file_data.values() if target in d["targets"] and not d["path"].endswith(".swift")]

# ── Build the pbxproj string ──────────────────────────────────────────────────

def pbx_file_ref(d):
    return f'\t\t{d["ref"]} = {{isa = PBXFileReference; lastKnownFileType = sourcecode.swift; name = "{d["name"]}"; path = "{d["path"]}"; sourceTree = "<group>"; }};'

def pbx_plist_ref(d):
    return f'\t\t{d["ref"]} = {{isa = PBXFileReference; lastKnownFileType = text.plist.xml; name = "{d["name"]}"; path = "{d["path"]}"; sourceTree = "<group>"; }};'

def pbx_build_file(d, target):
    bid = d["builds"][target]
    return f'\t\t{bid} = {{isa = PBXBuildFile; fileRef = {d["ref"]}; }};'

# ── Assemble ──────────────────────────────────────────────────────────────────

lines = []

lines.append("// !$*UTF8*$!")
lines.append("{")
lines.append("\tarchiveVersion = 1;")
lines.append("\tclasses = {")
lines.append("\t};")
lines.append("\tobjectVersion = 56;")
lines.append("\tobjects = {")
lines.append("")

# PBXBuildFile
lines.append("/* Begin PBXBuildFile section */")
for d in file_data.values():
    for t in d["targets"]:
        lines.append(pbx_build_file(d, t))
# Embed extensions build file (must be in this section)
lines.append(f'\t\t{EXT_EMBED_BF} = {{isa = PBXBuildFile; fileRef = {EXT_PRODUCT_REF}; settings = {{ATTRIBUTES = (RemoveHeadersOnCopy, ); }}; }};')
lines.append("/* End PBXBuildFile section */")
lines.append("")

# PBXFileReference
lines.append("/* Begin PBXFileReference section */")
for d in file_data.values():
    lines.append(pbx_file_ref(d))
for d in INFOPLIST_FILE_DATA.values():
    lines.append(pbx_plist_ref(d))
lines.append(f'\t\t{APP_PRODUCT_REF} = {{isa = PBXFileReference; explicitFileType = wrapper.application; includeInIndex = 0; path = Recall.app; sourceTree = BUILT_PRODUCTS_DIR; }};')
lines.append(f'\t\t{EXT_PRODUCT_REF} = {{isa = PBXFileReference; explicitFileType = "wrapper.app-extension"; includeInIndex = 0; path = RecallShareExtension.appex; sourceTree = BUILT_PRODUCTS_DIR; }};')
lines.append("/* End PBXFileReference section */")
lines.append("")

# PBXGroup
lines.append("/* Begin PBXGroup section */")

def group(gid, name, children, path=None):
    res = [f'\t\t{gid} = {{']
    res.append('\t\t\tisa = PBXGroup;')
    res.append('\t\t\tchildren = (')
    for c in children:
        res.append(f'\t\t\t\t{c},')
    res.append('\t\t\t);')
    if path:
        res.append(f'\t\t\tpath = "{path}";')
    else:
        res.append(f'\t\t\tname = "{name}";')
    res.append('\t\t\tsourceTree = "<group>";')
    res.append('\t\t};')
    return '\n'.join(res)

# Sub-groups
design_refs = [d["ref"] for d in file_data.values() if "DesignSystem" in d["path"]]
shared_refs  = [d["ref"] for d in file_data.values() if "/Shared/" in d["path"]]
models_refs  = [d["ref"] for d in file_data.values() if "/Models/" in d["path"]]
persist_refs = [d["ref"] for d in file_data.values() if "/Persistence/" in d["path"]]
service_refs = [d["ref"] for d in file_data.values() if "/Services/" in d["path"]]
vm_refs      = [d["ref"] for d in file_data.values() if "/ViewModels/" in d["path"]]
view_refs    = [d["ref"] for d in file_data.values() if "/Views/" in d["path"]]
res_refs_list = [d["ref"] for d in INFOPLIST_FILE_DATA.values() if "/Resources/" in d["path"]]
app_root_refs = [d["ref"] for d in file_data.values() if d["path"] == "Recall/RecallApp.swift"]
ext_refs = [d["ref"] for d in file_data.values() if d["path"].startswith("RecallShareExtension/")]
ext_refs += [d["ref"] for d in INFOPLIST_FILE_DATA.values() if d["path"].startswith("RecallShareExtension/")]

lines.append(group(DESIGN_GROUP,    "DesignSystem",  design_refs))
lines.append(group(SHARED_GROUP,    "Shared",        shared_refs))
lines.append(group(MODELS_GROUP,    "Models",        models_refs))
lines.append(group(PERSIST_GROUP,   "Persistence",   persist_refs))
lines.append(group(SERVICES_GROUP,  "Services",      service_refs))
lines.append(group(VIEWMODELS_GROUP,"ViewModels",    vm_refs))
lines.append(group(VIEWS_GROUP,     "Views",         view_refs))
lines.append(group(RESOURCES_GROUP, "Resources",     res_refs_list))
lines.append(group(RECALL_GROUP,    "Recall",        app_root_refs + [DESIGN_GROUP, SHARED_GROUP, MODELS_GROUP, PERSIST_GROUP, SERVICES_GROUP, VIEWMODELS_GROUP, VIEWS_GROUP, RESOURCES_GROUP]))
lines.append(group(EXT_GROUP,       "RecallShareExtension", ext_refs))
lines.append(group(PRODUCTS_ID,     "Products",      [APP_PRODUCT_REF, EXT_PRODUCT_REF]))
lines.append(group(MAIN_GROUP_ID,   "",              [RECALL_GROUP, EXT_GROUP, PRODUCTS_ID]))
lines.append("/* End PBXGroup section */")
lines.append("")

# PBXNativeTarget
def native_target(tid, name, product_ref, sources_phase, resources_phase, extra_phases, config_list, deps=[]):
    r = [f'\t\t{tid} = {{']
    r.append('\t\t\tisa = PBXNativeTarget;')
    r.append(f'\t\t\tbuildConfigurationList = {config_list};')
    r.append('\t\t\tbuildPhases = (')
    r.append(f'\t\t\t\t{sources_phase},')
    r.append(f'\t\t\t\t{resources_phase},')
    for ep in extra_phases:
        r.append(f'\t\t\t\t{ep},')
    r.append('\t\t\t);')
    r.append('\t\t\tbuildRules = ();')
    r.append('\t\t\tdependencies = (')
    for d in deps:
        r.append(f'\t\t\t\t{d},')
    r.append('\t\t\t);')
    r.append(f'\t\t\tname = "{name}";')
    r.append(f'\t\t\tproductName = "{name}";')
    r.append(f'\t\t\tproductReference = {product_ref};')
    r.append(f'\t\t\tproductType = "com.apple.product-type.{"app-extension" if "Extension" in name else "application"}";')
    r.append('\t\t};')
    return '\n'.join(r)

EXT_DEP_PROXY = make_id()
EXT_CONTAINER_PROXY = make_id()
EXT_EMBED_BUILD_FILE = make_id()

lines.append("/* Begin PBXNativeTarget section */")
lines.append(native_target(APP_TARGET_ID, "Recall",     APP_PRODUCT_REF, APP_SOURCES_PHASE, APP_RESOURCES_PHASE, [APP_EMBED_EXT_PHASE], APP_BUILD_CONFIG_LIST, [EXT_DEP_PROXY]))
lines.append(native_target(EXT_TARGET_ID, "RecallShareExtension", EXT_PRODUCT_REF, EXT_SOURCES_PHASE, EXT_RESOURCES_PHASE, [], EXT_BUILD_CONFIG_LIST))
lines.append("/* End PBXNativeTarget section */")
lines.append("")

# Dependency proxy
lines.append("/* Begin PBXTargetDependency section */")
lines.append(f'\t\t{EXT_DEP_PROXY} = {{isa = PBXTargetDependency; target = {EXT_TARGET_ID}; targetProxy = {EXT_CONTAINER_PROXY}; }};')
lines.append("/* End PBXTargetDependency section */")
lines.append("")
lines.append("/* Begin PBXContainerItemProxy section */")
lines.append(f'\t\t{EXT_CONTAINER_PROXY} = {{isa = PBXContainerItemProxy; containerPortal = {PROJECT_ID}; proxyType = 1; remoteGlobalIDString = {EXT_TARGET_ID}; remoteInfo = RecallShareExtension; }};')
lines.append("/* End PBXContainerItemProxy section */")
lines.append("")

# PBXProject
lines.append("/* Begin PBXProject section */")
lines.append(f'\t\t{PROJECT_ID} = {{')
lines.append('\t\t\tisa = PBXProject;')
lines.append('\t\t\tattributes = {')
lines.append('\t\t\t\tLastUpgradeCheck = 1500;')
lines.append(f'\t\t\t\tTargetAttributes = {{}};')
lines.append('\t\t\t};')
lines.append(f'\t\t\tbuildConfigurationList = {PROJ_BUILD_CONFIG_LIST};')
lines.append('\t\t\tcompatibilityVersion = "Xcode 14.0";')
lines.append('\t\t\tdevelopmentRegion = en;')
lines.append('\t\t\thasScannedForEncodings = 0;')
lines.append('\t\t\tknownRegions = (en, Base);')
lines.append(f'\t\t\tmainGroup = {MAIN_GROUP_ID};')
lines.append(f'\t\t\tproductRefGroup = {PRODUCTS_ID};')
lines.append('\t\t\tprojectDirPath = "";')
lines.append('\t\t\tprojectRoot = "";')
lines.append('\t\t\ttargets = (')
lines.append(f'\t\t\t\t{APP_TARGET_ID},')
lines.append(f'\t\t\t\t{EXT_TARGET_ID},')
lines.append('\t\t\t);')
lines.append('\t\t};')
lines.append("/* End PBXProject section */")
lines.append("")

# Source phases
def source_phase(pid, build_ids):
    r = [f'\t\t{pid} = {{']
    r.append('\t\t\tisa = PBXSourcesBuildPhase;')
    r.append('\t\t\tbuildActionMask = 2147483647;')
    r.append('\t\t\tfiles = (')
    for b in build_ids:
        r.append(f'\t\t\t\t{b},')
    r.append('\t\t\t);')
    r.append('\t\t\trunOnlyForDeploymentPostprocessing = 0;')
    r.append('\t\t};')
    return '\n'.join(r)

def resources_phase(pid, build_ids):
    r = [f'\t\t{pid} = {{']
    r.append('\t\t\tisa = PBXResourcesBuildPhase;')
    r.append('\t\t\tbuildActionMask = 2147483647;')
    r.append('\t\t\tfiles = (')
    for b in build_ids:
        r.append(f'\t\t\t\t{b},')
    r.append('\t\t\t);')
    r.append('\t\t\trunOnlyForDeploymentPostprocessing = 0;')
    r.append('\t\t};')
    return '\n'.join(r)

app_swift_builds = [d["builds"]["app"] for d in file_data.values() if "app" in d["targets"] and d["path"].endswith(".swift")]
app_res_builds   = [d["builds"]["app"] for d in file_data.values() if "app" in d["targets"] and not d["path"].endswith(".swift")]
ext_swift_builds = [d["builds"]["ext"] for d in file_data.values() if "ext" in d["targets"] and d["path"].endswith(".swift")]
ext_res_builds   = [d["builds"]["ext"] for d in file_data.values() if "ext" in d["targets"] and not d["path"].endswith(".swift")]

lines.append("/* Begin PBXSourcesBuildPhase section */")
lines.append(source_phase(APP_SOURCES_PHASE, app_swift_builds))
lines.append(source_phase(EXT_SOURCES_PHASE, ext_swift_builds))
lines.append("/* End PBXSourcesBuildPhase section */")
lines.append("")

lines.append("/* Begin PBXResourcesBuildPhase section */")
lines.append(resources_phase(APP_RESOURCES_PHASE, app_res_builds))
lines.append(resources_phase(EXT_RESOURCES_PHASE, ext_res_builds))
lines.append("/* End PBXResourcesBuildPhase section */")
lines.append("")

# Embed extensions phase
EXT_EMBED_BF = make_id()
lines.append("/* Begin PBXCopyFilesBuildPhase section */")
lines.append(f'\t\t{APP_EMBED_EXT_PHASE} = {{')
lines.append('\t\t\tisa = PBXCopyFilesBuildPhase;')
lines.append('\t\t\tbuildActionMask = 2147483647;')
lines.append('\t\t\tdstPath = "";')
lines.append('\t\t\tdstSubfolderSpec = 13;')
lines.append('\t\t\tfiles = (')
lines.append(f'\t\t\t\t{EXT_EMBED_BF},')
lines.append('\t\t\t);')
lines.append('\t\t\tname = "Embed Foundation Extensions";')
lines.append('\t\t\trunOnlyForDeploymentPostprocessing = 0;')
lines.append('\t\t};')
lines.append("/* End PBXCopyFilesBuildPhase section */")
lines.append("")

# NOTE: EXT_EMBED_BF is declared here but placed in the main PBXBuildFile section above at generation time.
# We need to inject it retroactively - handled by moving it into the build phase references only.
lines.append("")

# Build configurations
BASE_APP_SETTINGS = {
    "ALWAYS_SEARCH_USER_PATHS": "NO",
    "CLANG_ENABLE_MODULES": "YES",
    "CODE_SIGN_STYLE": "Automatic",
    "CURRENT_PROJECT_VERSION": "1",
    "INFOPLIST_FILE": '"Recall/Resources/Info.plist"',
    "IPHONEOS_DEPLOYMENT_TARGET": "17.0",
    "LD_RUNPATH_SEARCH_PATHS": '"$(inherited) @executable_path/Frameworks"',
    "MARKETING_VERSION": "1.0",
    "PRODUCT_BUNDLE_IDENTIFIER": '"com.recall.app"',
    "PRODUCT_NAME": '"$(TARGET_NAME)"',
    "SWIFT_EMIT_LOC_STRINGS": "YES",
    "SWIFT_STRICT_CONCURRENCY": "complete",
    "SWIFT_VERSION": '"5.0"',
    "TARGETED_DEVICE_FAMILY": '"1"',
    "CODE_SIGN_ENTITLEMENTS": '"Recall/Resources/Recall.entitlements"',
    "ASSETCATALOG_COMPILER_APPICON_NAME": "AppIcon",
}

BASE_EXT_SETTINGS = {
    "ALWAYS_SEARCH_USER_PATHS": "NO",
    "CLANG_ENABLE_MODULES": "YES",
    "CODE_SIGN_STYLE": "Automatic",
    "CURRENT_PROJECT_VERSION": "1",
    "INFOPLIST_FILE": '"RecallShareExtension/Info.plist"',
    "IPHONEOS_DEPLOYMENT_TARGET": "17.0",
    "LD_RUNPATH_SEARCH_PATHS": '"$(inherited) @executable_path/Frameworks @executable_path/../../Frameworks"',
    "MARKETING_VERSION": "1.0",
    "PRODUCT_BUNDLE_IDENTIFIER": '"com.recall.app.share"',
    "PRODUCT_NAME": '"$(TARGET_NAME)"',
    "SKIP_INSTALL": "YES",
    "APPLICATION_EXTENSION_API_ONLY": "YES",
    "SWIFT_EMIT_LOC_STRINGS": "YES",
    "SWIFT_STRICT_CONCURRENCY": "complete",
    "SWIFT_VERSION": '"5.0"',
    "TARGETED_DEVICE_FAMILY": '"1"',
    "CODE_SIGN_ENTITLEMENTS": '"RecallShareExtension/RecallShareExtension.entitlements"',
}

BASE_PROJ_SETTINGS = {
    "ALWAYS_SEARCH_USER_PATHS": "NO",
    "CLANG_ANALYZER_NONNULL": "YES",
    "CLANG_ENABLE_MODULES": "YES",
    "CLANG_ENABLE_OBJC_ARC": "YES",
    "GCC_C_LANGUAGE_STANDARD": "gnu11",
    "IPHONEOS_DEPLOYMENT_TARGET": "17.0",
    "MTL_ENABLE_DEBUG_INFO": "INCLUDE_SOURCE",
    "SDKROOT": "iphoneos",
    "SWIFT_ACTIVE_COMPILATION_CONDITIONS": "DEBUG",
    "SWIFT_OPTIMIZATION_LEVEL": '"-Onone"',
}

def build_cfg(cfg_id, name, settings):
    r = [f'\t\t{cfg_id} = {{']
    r.append('\t\t\tisa = XCBuildConfiguration;')
    r.append('\t\t\tbuildSettings = {')
    for k, v in settings.items():
        r.append(f'\t\t\t\t{k} = {v};')
    r.append('\t\t\t};')
    r.append(f'\t\t\tname = {name};')
    r.append('\t\t};')
    return '\n'.join(r)

def cfg_list(list_id, debug_id, release_id):
    r = [f'\t\t{list_id} = {{']
    r.append('\t\t\tisa = XCConfigurationList;')
    r.append('\t\t\tbuildConfigurations = (')
    r.append(f'\t\t\t\t{debug_id},')
    r.append(f'\t\t\t\t{release_id},')
    r.append('\t\t\t);')
    r.append('\t\t\tdefaultConfigurationIsVisible = 0;')
    r.append('\t\t\tdefaultConfigurationName = Release;')
    r.append('\t\t};')
    return '\n'.join(r)

app_debug_settings  = {**BASE_APP_SETTINGS, "DEBUG_INFORMATION_FORMAT": "dwarf", "SWIFT_ACTIVE_COMPILATION_CONDITIONS": "DEBUG", "SWIFT_OPTIMIZATION_LEVEL": '"-Onone"'}
app_release_settings = {**BASE_APP_SETTINGS, "DEBUG_INFORMATION_FORMAT": '"dwarf-with-dsym"', "SWIFT_OPTIMIZATION_LEVEL": '"-O"', "VALIDATE_PRODUCT": "YES"}
ext_debug_settings  = {**BASE_EXT_SETTINGS,  "DEBUG_INFORMATION_FORMAT": "dwarf", "SWIFT_ACTIVE_COMPILATION_CONDITIONS": "DEBUG", "SWIFT_OPTIMIZATION_LEVEL": '"-Onone"'}
ext_release_settings = {**BASE_EXT_SETTINGS, "DEBUG_INFORMATION_FORMAT": '"dwarf-with-dsym"', "SWIFT_OPTIMIZATION_LEVEL": '"-O"', "VALIDATE_PRODUCT": "YES"}
proj_debug_settings  = {**BASE_PROJ_SETTINGS}
proj_release_settings = {k: v for k, v in BASE_PROJ_SETTINGS.items() if k not in ("MTL_ENABLE_DEBUG_INFO", "SWIFT_ACTIVE_COMPILATION_CONDITIONS")}
proj_release_settings["SWIFT_OPTIMIZATION_LEVEL"] = '"-O"'

lines.append("/* Begin XCBuildConfiguration section */")
lines.append(build_cfg(APP_DEBUG_CFG,   "Debug",   app_debug_settings))
lines.append(build_cfg(APP_RELEASE_CFG, "Release", app_release_settings))
lines.append(build_cfg(EXT_DEBUG_CFG,   "Debug",   ext_debug_settings))
lines.append(build_cfg(EXT_RELEASE_CFG, "Release", ext_release_settings))
lines.append(build_cfg(PROJ_DEBUG_CFG,  "Debug",   proj_debug_settings))
lines.append(build_cfg(PROJ_RELEASE_CFG,"Release", proj_release_settings))
lines.append("/* End XCBuildConfiguration section */")
lines.append("")

lines.append("/* Begin XCConfigurationList section */")
lines.append(cfg_list(APP_BUILD_CONFIG_LIST,  APP_DEBUG_CFG,  APP_RELEASE_CFG))
lines.append(cfg_list(EXT_BUILD_CONFIG_LIST,  EXT_DEBUG_CFG,  EXT_RELEASE_CFG))
lines.append(cfg_list(PROJ_BUILD_CONFIG_LIST, PROJ_DEBUG_CFG, PROJ_RELEASE_CFG))
lines.append("/* End XCConfigurationList section */")
lines.append("")

lines.append("\t};")
lines.append(f"\trootObject = {PROJECT_ID};")
lines.append("}")

# ── Write out ─────────────────────────────────────────────────────────────────
xcodeproj = os.path.join(BASE, "Recall.xcodeproj")
os.makedirs(xcodeproj, exist_ok=True)
pbxpath = os.path.join(xcodeproj, "project.pbxproj")
with open(pbxpath, "w") as f:
    f.write("\n".join(lines))

print(f"✓ Written {pbxpath}")
print("Open Recall.xcodeproj in Xcode, then:")
print("  1. Set your Development Team in Signing & Capabilities for both targets.")
print("  2. Add the App Group 'group.com.recall.app' in Signing & Capabilities → + Capability.")
print("  3. Build & run.")
