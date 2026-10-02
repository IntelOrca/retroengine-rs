// Script compiler oracle harness.
//
// Fetches (via `run.sh`) the upstream `Script.cpp` from RSDKv4-Decompilation at a pinned commit,
// extracts its compiler half, and links it against the stubs in `stubs.hpp` so the reference
// compiler can be run on the shipped script sources. The harness writes the same canonical
// `_Bytecode` container that `retro_script::write_bytecode` produces, so outputs can be compared
// byte-for-byte.
//
// This file contains no upstream code.

#include "stubs.hpp"

#include <algorithm>
#include <filesystem>
#include <string>
#include <vector>

EngineStub Engine;
FileInfo *currentFile = nullptr;
int gameMenu[1] = { 0 };
SceneInfo sceneInfo;
char typeNames[OBJECT_COUNT][0x40];
char sfxNames[SFX_COUNT][0x20];
char globalVariableNames[GLOBALVAR_COUNT][0x20];
int globalVariablesCount = 0;
Achievement achievements[ACHIEVEMENT_COUNT];
int achievementCount = 0;
static char playerNameStorage[PLAYER_COUNT][0x20] = { "SONIC", "TAILS", "KNUCKLES", "SONIC AND TAILS" };
char *playerNames[PLAYER_COUNT] = { playerNameStorage[0], playerNameStorage[1], playerNameStorage[2],
                                    playerNameStorage[3] };

void PrintLog(const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    fprintf(stderr, "\n");
    va_end(args);
}
void SetupTextMenu(void *, int) {}
void AddTextMenuEntry(void *, const char *text) {
    fprintf(stderr, "MENU: %s\n", text);
}
int GetSceneID(char, const char *) {
    return 0;
}

int FindStringToken(const char *string, const char *token, char stopID) {
    int tokenCharID = 0, stringCharID = 0, foundTokenID = 0;
    while (string[stringCharID]) {
        tokenCharID = 0;
        bool tokenMatch = true;
        while (token[tokenCharID]) {
            if (!string[tokenCharID + stringCharID])
                return -1;
            if (string[tokenCharID + stringCharID] != token[tokenCharID])
                tokenMatch = false;
            ++tokenCharID;
        }
        if (tokenMatch && ++foundTokenID == stopID)
            return stringCharID;
        ++stringCharID;
    }
    return -1;
}

static std::vector<byte> pendingSource;
static int gFilePos = 0;
bool LoadFile(const char *path, FileInfo *info) {
    (void)path;
    if (pendingSource.empty())
        return false;
    info->data = pendingSource;
    info->readPos = 0;
    gFilePos = 0;
    currentFile = info;
    return true;
}
void FileRead(void *dest, int size) {
    if (!currentFile)
        return;
    if (gFilePos + size > (int)currentFile->data.size())
        size = (int)currentFile->data.size() - gFilePos;
    if (size > 0)
        memcpy(dest, currentFile->data.data() + gFilePos, size);
    gFilePos += size;
}
void CloseFile() {
    currentFile = nullptr;
}
bool ReachedEndOfFile() {
    return !currentFile || gFilePos >= (int)currentFile->data.size();
}
int GetFilePosition() {
    return gFilePos;
}
void SetFilePosition(int pos) {
    gFilePos = pos;
}

// `run.sh` generates `compiler_section.cpp` in the build directory and passes `-I<build>`.
#include "compiler_section.cpp"

// ---------------------------------------------------------------------------
// Minimal GameConfig.bin / StageConfig.bin readers
// ---------------------------------------------------------------------------

struct Reader {
    const std::vector<byte> &data;
    size_t pos = 0;
    explicit Reader(const std::vector<byte> &d) : data(d) {}
    byte u8() {
        return data[pos++];
    }
    std::string str() {
        byte n = u8();
        std::string s((const char *)data.data() + pos, n);
        pos += n;
        return s;
    }
    int i32() {
        int v;
        memcpy(&v, data.data() + pos, 4);
        pos += 4;
        return v;
    }
};

static std::vector<byte> readAll(const std::string &path) {
    FILE *f = fopen(path.c_str(), "rb");
    if (!f) {
        fprintf(stderr, "cannot open %s\n", path.c_str());
        exit(1);
    }
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    std::vector<byte> data((size_t)size);
    if (size && fread(data.data(), 1, (size_t)size, f) != (size_t)size) {
        fprintf(stderr, "short read on %s\n", path.c_str());
        exit(1);
    }
    fclose(f);
    return data;
}

static void stripTo(char *dest, const std::string &src) {
    int pos = 0;
    for (char c : src)
        if (c != ' ')
            dest[pos++] = c;
    dest[pos] = 0;
}

struct StageLists {
    std::string name;
    bool loadGlobal = true;
    std::vector<std::string> scripts;
    std::vector<std::string> objectNames;
    std::vector<std::string> sfx;
};

struct GameData {
    std::string root;
    std::vector<std::string> globalScripts;
    std::vector<std::string> globalObjectNames;
    std::vector<std::string> globalVariables;
    std::vector<std::string> globalSfx;
};

static GameData loadGame(const std::string &root) {
    GameData game;
    game.root = root;
    auto bytes = readAll(root + "/Data/Game/GameConfig.bin");
    Reader r(bytes);
    r.str();
    r.str();
    r.pos += 0x60 * 3;
    int objCount = r.u8();
    std::vector<std::string> names;
    for (int i = 0; i < objCount; ++i)
        names.push_back(r.str());
    game.globalObjectNames = names;
    for (int i = 0; i < objCount; ++i)
        game.globalScripts.push_back(r.str());
    int varCount = r.u8();
    for (int i = 0; i < varCount; ++i) {
        std::string name = r.str();
        r.i32();
        game.globalVariables.push_back(name);
    }
    int sfxCount = r.u8();
    std::vector<std::string> sfxNames;
    for (int i = 0; i < sfxCount; ++i)
        sfxNames.push_back(r.str());
    for (int i = 0; i < sfxCount; ++i)
        r.str();
    game.globalSfx = sfxNames;
    return game;
}

static StageLists loadStage(const GameData &game, const std::string &name) {
    StageLists stage;
    stage.name = name;
    auto bytes = readAll(game.root + "/Data/Stages/" + name + "/StageConfig.bin");
    Reader r(bytes);
    stage.loadGlobal = r.u8() != 0;
    r.pos += 0x20 * 3;
    int sfxCount = r.u8();
    for (int i = 0; i < sfxCount; ++i)
        stage.sfx.push_back(r.str());
    for (int i = 0; i < sfxCount; ++i)
        r.str();
    int objCount = r.u8();
    for (int i = 0; i < objCount; ++i)
        stage.objectNames.push_back(r.str());
    for (int i = 0; i < objCount; ++i)
        stage.scripts.push_back(r.str());
    return stage;
}

// ---------------------------------------------------------------------------
// Script state setup and canonical serialisation
// ---------------------------------------------------------------------------

struct CompileState {
    int codeStart = 0;
    int jumpStart = 0;
    std::vector<int> scriptIDs;
};

static void clearScriptData() {
    memset(scriptCode, 0, sizeof(scriptCode));
    memset(jumpTable, 0, sizeof(jumpTable));
    memset(jumpTableStack, 0, sizeof(jumpTableStack));
    memset(functionStack, 0, sizeof(functionStack));
    memset(foreachStack, -1, sizeof(foreachStack));
    scriptCodePos = 0;
    scriptCodeOffset = 0;
    jumpTablePos = 0;
    jumpTableOffset = 0;
    jumpTableStackPos = 0;
    functionStackPos = 0;
    foreachStackPos = 0;
    scriptFunctionCount = 0;
    lineID = 0;
    scriptValueListCount = COMMON_SCRIPT_VAR_COUNT;
    for (int v = COMMON_SCRIPT_VAR_COUNT; v < SCRIPT_VAR_COUNT; ++v)
        MEM_ZERO(scriptValueList[v]);
    for (int o = 0; o < OBJECT_COUNT; ++o) {
        objectScriptList[o].eventUpdate.scriptCodePtr = SCRIPTCODE_COUNT - 1;
        objectScriptList[o].eventUpdate.jumpTablePtr = JUMPTABLE_COUNT - 1;
        objectScriptList[o].eventDraw.scriptCodePtr = SCRIPTCODE_COUNT - 1;
        objectScriptList[o].eventDraw.jumpTablePtr = JUMPTABLE_COUNT - 1;
        objectScriptList[o].eventStartup.scriptCodePtr = SCRIPTCODE_COUNT - 1;
        objectScriptList[o].eventStartup.jumpTablePtr = JUMPTABLE_COUNT - 1;
    }
    for (int f = 0; f < FUNCTION_COUNT; ++f) {
        scriptFunctionList[f].ptr.scriptCodePtr = SCRIPTCODE_COUNT - 1;
        scriptFunctionList[f].ptr.jumpTablePtr = JUMPTABLE_COUNT - 1;
    }
}

static void parseFiles(const GameData &game, const std::vector<std::string> &scripts,
                       std::vector<int> &ids, int &nextID) {
    for (const std::string &script : scripts) {
        pendingSource = readAll(game.root + "/Data/Scripts/" + script);
        ParseScriptFile((char *)script.c_str(), nextID);
        ids.push_back(nextID);
        nextID++;
        if (Engine.gameMode == ENGINE_SCRIPTERROR) {
            fprintf(stderr, "script error in %s\n", script.c_str());
            exit(1);
        }
    }
}

/// Mirrors `retro_script::write_bytecode`: byte blocks for 0..=255, 32-bit blocks otherwise.
static void writeBlocks(const int *values, int count, FILE *out) {
    int i = 0;
    while (i < count) {
        bool byteBlock = values[i] >= 0 && values[i] <= 255;
        int start = i;
        while (i < count && i - start < 0x7F && ((values[i] >= 0 && values[i] <= 255) == byteBlock))
            i++;
        int n = i - start;
        if (byteBlock) {
            fputc(n, out);
            for (int k = start; k < i; k++)
                fputc(values[k] & 0xFF, out);
        }
        else {
            fputc(0x80 | n, out);
            for (int k = start; k < i; k++) {
                uint32_t v = (uint32_t)values[k];
                fwrite(&v, 4, 1, out);
            }
        }
    }
}

static void writeCanonical(const CompileState &state, const std::string &outPath) {
    FILE *out = fopen(outPath.c_str(), "wb");
    if (!out) {
        fprintf(stderr, "cannot write %s\n", outPath.c_str());
        exit(1);
    }
    int codeCount = scriptCodePos - state.codeStart;
    int jumpCount = jumpTablePos - state.jumpStart;
    uint32_t codeSize = (uint32_t)codeCount;
    fwrite(&codeSize, 4, 1, out);
    writeBlocks(scriptCode + state.codeStart, codeCount, out);
    uint32_t jumpSize = (uint32_t)jumpCount;
    fwrite(&jumpSize, 4, 1, out);
    writeBlocks(jumpTable + state.jumpStart, jumpCount, out);

    uint16_t scriptCount = (uint16_t)state.scriptIDs.size();
    fwrite(&scriptCount, 2, 1, out);
    for (int id : state.scriptIDs) {
        int v = objectScriptList[id].eventUpdate.scriptCodePtr;
        fwrite(&v, 4, 1, out);
        v = objectScriptList[id].eventDraw.scriptCodePtr;
        fwrite(&v, 4, 1, out);
        v = objectScriptList[id].eventStartup.scriptCodePtr;
        fwrite(&v, 4, 1, out);
    }
    for (int id : state.scriptIDs) {
        int v = objectScriptList[id].eventUpdate.jumpTablePtr;
        fwrite(&v, 4, 1, out);
        v = objectScriptList[id].eventDraw.jumpTablePtr;
        fwrite(&v, 4, 1, out);
        v = objectScriptList[id].eventStartup.jumpTablePtr;
        fwrite(&v, 4, 1, out);
    }
    uint16_t functionCount = (uint16_t)scriptFunctionCount;
    fwrite(&functionCount, 2, 1, out);
    for (int i = 0; i < functionCount; ++i) {
        int v = scriptFunctionList[i].ptr.scriptCodePtr;
        fwrite(&v, 4, 1, out);
    }
    for (int i = 0; i < functionCount; ++i) {
        int v = scriptFunctionList[i].ptr.jumpTablePtr;
        fwrite(&v, 4, 1, out);
    }
    fclose(out);
    fprintf(stderr, "wrote %s (%d code words, %d jump words, %d scripts, %d functions)\n",
            outPath.c_str(), codeCount, jumpCount, (int)scriptCount, (int)functionCount);
}

static void registerGlobalSymbols(const GameData &game) {
    globalVariablesCount = (int)game.globalVariables.size();
    for (size_t i = 0; i < game.globalVariables.size(); ++i)
        StrCopy(globalVariableNames[i], game.globalVariables[i].c_str());
    memset(typeNames, 0, sizeof(typeNames));
    memset(sfxNames, 0, sizeof(sfxNames));
    stripTo(typeNames[0], "BlankObject");
    int typeCount = 1;
    for (const auto &name : game.globalObjectNames)
        stripTo(typeNames[typeCount++], name);
    int sfxCount = 0;
    for (const auto &name : game.globalSfx)
        stripTo(sfxNames[sfxCount++], name);
}

/// Compiles one group and writes it in the canonical bytecode container.
static void compileGroup(const GameData &game, const StageLists *stage, const std::string &outPath) {
    registerGlobalSymbols(game);
    clearScriptData();

    CompileState state;
    int nextID = 1;
    std::vector<int> globalIDs;
    bool isGlobal = stage == nullptr;
    bool loadGlobal = isGlobal || stage->loadGlobal;

    if (loadGlobal)
        parseFiles(game, game.globalScripts, globalIDs, nextID);
    if (isGlobal) {
        state.scriptIDs = globalIDs;
    }
    else {
        if (!loadGlobal) {
            // Standalone stages never see the global object names.
            memset(typeNames, 0, sizeof(typeNames));
            stripTo(typeNames[0], "BlankObject");
            int typeCount = 1;
            for (const auto &name : stage->objectNames)
                stripTo(typeNames[typeCount++], name);
        }
        else {
            int typeCount = 1;
            while (typeNames[typeCount][0])
                typeCount++;
            for (const auto &name : stage->objectNames)
                stripTo(typeNames[typeCount++], name);
        }
        int sfxCount = 0;
        while (sfxNames[sfxCount][0])
            sfxCount++;
        for (const auto &name : stage->sfx)
            stripTo(sfxNames[sfxCount++], name);

        state.codeStart = scriptCodePos;
        state.jumpStart = jumpTablePos;
        std::vector<int> stageIDs;
        parseFiles(game, stage->scripts, stageIDs, nextID);
        state.scriptIDs = stageIDs;
    }
    writeCanonical(state, outPath);
}

int main(int argc, char **argv) {
    std::string platform = "origins";
    std::vector<std::string> args;
    for (int i = 1; i < argc; ++i) {
        std::string arg = argv[i];
        if (arg == "origins" || arg == "standalone")
            platform = arg;
        else
            args.push_back(arg);
    }
    if (args.size() < 3) {
        fprintf(stderr,
                "usage: script-oracle <gameRoot> <GLOBAL|stageFolder|--all> <out.bin|outDir> "
                "[origins|standalone]\n");
        return 2;
    }
    std::string root = args[0];
    std::string group = args[1];
    std::string out = args[2];
    Engine.releaseType = platform == "standalone" ? "USE_STANDALONE" : "USE_ORIGINS";

    GameData game = loadGame(root);
    if (group == "--all") {
        std::filesystem::create_directories(out);
        compileGroup(game, nullptr, out + "/GlobalCode.bin");
        std::vector<std::string> names;
        for (const auto &entry : std::filesystem::directory_iterator(root + "/Data/Stages")) {
            if (entry.is_directory())
                names.push_back(entry.path().filename().string());
        }
        std::sort(names.begin(), names.end());
        for (const std::string &name : names) {
            if (!std::filesystem::exists(root + "/Data/Stages/" + name + "/StageConfig.bin"))
                continue;
            StageLists stage = loadStage(game, name);
            compileGroup(game, &stage, out + "/" + name + ".bin");
        }
    }
    else if (group == "GLOBAL") {
        compileGroup(game, nullptr, out);
    }
    else {
        StageLists stage = loadStage(game, group);
        compileGroup(game, &stage, out);
    }
    return 0;
}
