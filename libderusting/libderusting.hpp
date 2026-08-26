#pragma once

/**
 * @file libderusting.hpp
 * @brief Calling Rust from C/C++
 */
#ifdef __cplusplus
extern "C" {
#endif
  /**
   * @brief Our entrypoint to our Rust library.
   */
  void derusting_main(void);
#ifdef __cplusplus
}
#endif
