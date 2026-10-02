#ifndef RETRO_SCRIPT_ORACLE_STUBS_HPP
#define RETRO_SCRIPT_ORACLE_STUBS_HPP

// Minimal stand-ins for the engine surface that the extracted upstream compiler section needs.
// This file is part of the oracle harness; it contains no upstream code. `run.sh` fetches the
// reference `Script.cpp` at run time and includes it after this header.

#include <cstdarg>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

typedef unsigned char byte;
typedef unsigned short ushort;
typedef unsigned int uint;

#define RETRO_USE_COMPILER (1)
#define RETRO_REV00 (0)
#define RETRO_REV01 (1)
#define RETRO_REV02 (1)
#define RETRO_REV03 (1)
#define RETRO_USE_HAPTICS (1)
#define RETRO_USE_ORIGINAL_CODE (0)
#define RETRO_USE_NETWORKING (0)
#define RETRO_USE_MOD_LOADER (0)

#define SCRIPTCODE_COUNT (0x40000)
#define JUMPTABLE_COUNT (0x4000)
#define FUNCTION_COUNT (0x200)
#define JUMPSTACK_COUNT (0x400)
#define FUNCSTACK_COUNT (0x400)
#define FORSTACK_COUNT (0x400)
#define OBJECT_COUNT (0x400)
#define GLOBALVAR_COUNT (0x100)
#define SFX_COUNT (0x100)
#define PLAYER_COUNT (4)
#define ACHIEVEMENT_COUNT (0x40)

#define ENGINE_SCRIPTERROR (0xFF)

#define MEM_ZERO(x) memset(&(x), 0, sizeof((x)))
#define MEM_ZEROP(x) memset((x), 0, sizeof(*(x)))

enum StageListNames {
    STAGELIST_PRESENTATION = 0,
    STAGELIST_REGULAR = 1,
    STAGELIST_BONUS = 2,
    STAGELIST_SPECIAL = 3
};

struct ScriptPtr {
    int scriptCodePtr;
    int jumpTablePtr;
};

struct ScriptFunction {
    byte access;
    char name[0x20];
    ScriptPtr ptr;
};

struct ObjectScript {
    int frameCount;
    int spriteSheetID;
    ScriptPtr eventUpdate;
    ScriptPtr eventDraw;
    ScriptPtr eventStartup;
    int frameListOffset;
    void *animFile;
};

struct ScriptEngine {
    int operands[0x10];
    int temp[8];
    int arrayPosition[9];
    int checkResult;
};

struct Achievement {
    char name[0x40];
};

struct SceneListEntry {
    char name[0x40];
};

struct SceneListCategory {
    int sceneOffsetStart;
    int sceneCount;
};

struct SceneInfo {
    SceneListCategory listCategory[4];
    SceneListEntry listData[0x100];
};

struct EngineStub {
    int gameMode = 0;
    const char *gamePlatform = "STANDARD";
    const char *gameRenderType = "HW_RENDERING";
    const char *gameHapticSetting = "NO_F_FEEDBACK";
    const char *releaseType = "USE_ORIGINS";
};
extern EngineStub Engine;

struct FileInfo {
    std::vector<byte> data;
    int readPos = 0;
};
extern FileInfo *currentFile;

extern int gameMenu[1];
extern SceneInfo sceneInfo;
extern char typeNames[OBJECT_COUNT][0x40];
extern char sfxNames[SFX_COUNT][0x20];
extern char globalVariableNames[GLOBALVAR_COUNT][0x20];
extern int globalVariablesCount;
extern Achievement achievements[ACHIEVEMENT_COUNT];
extern int achievementCount;
extern char *playerNames[PLAYER_COUNT];

void PrintLog(const char *fmt, ...);
void SetupTextMenu(void *menu, int value);
void AddTextMenuEntry(void *menu, const char *text);
int GetSceneID(char list, const char *name);
int FindStringToken(const char *string, const char *token, char stopID);

inline void StrCopy(char *dest, const char *src) {
    strcpy(dest, src);
}
inline void StrAdd(char *dest, const char *src) {
    strcat(dest, src);
}
inline int StrLength(const char *string) {
    return (int)strlen(string);
}
inline bool StrComp(const char *stringA, const char *stringB) {
    bool match = true, finished = false;
    while (!finished) {
        if (*stringA == *stringB || *stringA == *stringB + ' ' || *stringA == *stringB - ' ') {
            if (*stringA) {
                ++stringA;
                ++stringB;
            }
            else {
                finished = true;
            }
        }
        else {
            match = false;
            finished = true;
        }
    }
    return match;
}

bool ConvertStringToInteger(const char *text, int *value);
void AppendIntegerToString(char *text, int value);
void AppendIntegerToStringW(ushort *text, int value);
void CheckAliasText(char *text);
void CheckStaticText(char *text);
bool CheckTableText(char *text);
void ConvertArithmaticSyntax(char *text);
void ConvertConditionalStatement(char *text);
bool ConvertSwitchStatement(char *text);
void ConvertFunctionText(char *text);
void CheckCaseNumber(char *text);
bool ReadSwitchCase(char *text);
void ReadTableValues(char *text);
void CopyAliasStr(char *dest, char *text, bool arrayIndex);
bool CheckOpcodeType(char *text);
void ParseScriptFile(char *scriptName, int scriptID);

bool LoadFile(const char *path, FileInfo *info);
void FileRead(void *dest, int size);
void CloseFile();
bool ReachedEndOfFile();
int GetFilePosition();
void SetFilePosition(int pos);

#endif
