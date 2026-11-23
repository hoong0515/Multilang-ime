// Multilang IME Mozc helper: runs the Mozc engine in-process and talks over stdin/stdout.
//
// Protocol (binary, little endian): each message is a 4-byte length followed by a serialized
// protobuf. Requests are mozc::commands::Input, responses are mozc::commands::Output.
// The helper exits when stdin closes (the backend that started it went away).
//
// The user profile (learning data) directory comes from the MULTILANG_MOZC_PROFILE environment
// variable; without it Mozc's default directory is used.
//
// Copyright (C) 2026 Multilang IME contributors. GPL-3.0-or-later.
// Built inside the Mozc source tree (BSD-3-Clause, Copyright Google Inc.).

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <memory>
#include <string>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif  // _WIN32

#include "base/init_mozc.h"
#include "base/system_util.h"
#include "engine/engine_factory.h"
#include "protocol/commands.pb.h"
#include "session/session_handler.h"

namespace {

bool ReadExact(void* buf, size_t n) {
  return n == 0 || std::fread(buf, 1, n, stdin) == n;
}

bool ReadMessage(std::string* out) {
  uint8_t len_bytes[4];
  if (!ReadExact(len_bytes, 4)) return false;
  const uint32_t len = len_bytes[0] | (len_bytes[1] << 8) | (len_bytes[2] << 16) |
                       (static_cast<uint32_t>(len_bytes[3]) << 24);
  if (len > (64u << 20)) return false;  // 64 MiB: protocol error
  out->resize(len);
  return ReadExact(out->data(), len);
}

bool WriteMessage(const std::string& body) {
  const uint32_t len = static_cast<uint32_t>(body.size());
  const uint8_t len_bytes[4] = {static_cast<uint8_t>(len), static_cast<uint8_t>(len >> 8),
                                static_cast<uint8_t>(len >> 16), static_cast<uint8_t>(len >> 24)};
  if (std::fwrite(len_bytes, 1, 4, stdout) != 4) return false;
  if (!body.empty() && std::fwrite(body.data(), 1, body.size(), stdout) != body.size()) return false;
  return std::fflush(stdout) == 0;
}

}  // namespace

int main(int argc, char* argv[]) {
#ifdef _WIN32
  _setmode(_fileno(stdin), _O_BINARY);
  _setmode(_fileno(stdout), _O_BINARY);
#endif  // _WIN32
  if (const char* dir = std::getenv("MULTILANG_MOZC_PROFILE"); dir != nullptr && *dir != '\0') {
    mozc::SystemUtil::SetUserProfileDirectory(dir);
  }
  mozc::InitMozc(argv[0], &argc, &argv);

  auto engine = mozc::EngineFactory::Create();
  if (!engine.ok()) {
    std::fprintf(stderr, "mozc_stdio: engine init failed\n");
    return 1;
  }
  mozc::SessionHandler handler(*std::move(engine));

  std::string request, response;
  while (ReadMessage(&request)) {
    mozc::commands::Command command;
    if (!command.mutable_input()->ParseFromString(request)) {
      response.clear();  // invalid request: empty response, keep going
    } else {
      if (!handler.EvalCommand(&command)) break;
      if (!command.output().SerializeToString(&response)) response.clear();
    }
    if (!WriteMessage(response)) break;
  }
  return 0;
}
