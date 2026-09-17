import "@typespec/json-schema";

using TypeSpec.JsonSchema;

/**
 * Independently authored rate-limit policy authority.
 * This file is not generated from JSON Schema and must pass the parity gate.
 */
namespace Ores.Middleware.RateLimitV2;

enum RateLimitAlgorithmV2 {
  tokenBucket: "token-bucket",
  slidingWindowCounter: "sliding-window-counter",
  fixedWindow: "fixed-window",
  gcra: "gcra",
  concurrency: "concurrency",
}

enum RateLimitConsistency {
  strict: "strict",
  bounded: "bounded",
  advisory: "advisory",
}

enum RateLimitEnforcementMode {
  disabled: "disabled",
  audit: "audit",
  enforce: "enforce",
}

enum RateLimitFailureMode {
  failOpen: "fail-open",
  failClosed: "fail-closed",
  localOnly: "local-only",
}

enum RateLimitLayer {
  cloudflareEdge: "cloudflare-edge",
  kubernetesIngress: "kubernetes-ingress",
  serviceMesh: "service-mesh",
  application: "application",
  authorization: "authorization",
}

enum OperationClass {
  healthRead: "health-read",
  publicRead: "public-read",
  authAttempt: "auth-attempt",
  authRecovery: "auth-recovery",
  mutation: "mutation",
  paymentOrLedgerWrite: "payment-or-ledger-write",
  jobAdmission: "job-admission",
}

/**
 * Version-two policy used by application middleware and audit rollouts.
 * Conditional algorithm and consistency invariants are enforced independently
 * by Rust runtime validation and the peer JSON Schema authority.
 */
@TypeSpec.JsonSchema.extension("additionalProperties", false)
@TypeSpec.JsonSchema.extension("allOf", #[
  #{
    `if`: #{
      `properties`: #{
        `algorithm`: #{
          `const`: "token-bucket"
        }
      },
      `required`: #[
        "algorithm"
      ]
    },
    `then`: #{
      `properties`: #{
        `refillTokens`: #{},
        `refillIntervalMs`: #{}
      },
      `required`: #[
        "refillTokens",
        "refillIntervalMs"
      ],
      `not`: #{
        `properties`: #{
          `windowMs`: #{}
        },
        `required`: #[
          "windowMs"
        ]
      }
    }
  },
  #{
    `if`: #{
      `properties`: #{
        `algorithm`: #{
          `enum`: #[
            "sliding-window-counter",
            "fixed-window",
            "gcra"
          ]
        }
      },
      `required`: #[
        "algorithm"
      ]
    },
    `then`: #{
      `properties`: #{
        `windowMs`: #{}
      },
      `required`: #[
        "windowMs"
      ],
      `not`: #{
        `anyOf`: #[
          #{
            `properties`: #{
              `refillTokens`: #{}
            },
            `required`: #[
              "refillTokens"
            ]
          },
          #{
            `properties`: #{
              `refillIntervalMs`: #{}
            },
            `required`: #[
              "refillIntervalMs"
            ]
          }
        ]
      }
    }
  },
  #{
    `if`: #{
      `properties`: #{
        `algorithm`: #{
          `const`: "concurrency"
        }
      },
      `required`: #[
        "algorithm"
      ]
    },
    `then`: #{
      `not`: #{
        `anyOf`: #[
          #{
            `properties`: #{
              `windowMs`: #{}
            },
            `required`: #[
              "windowMs"
            ]
          },
          #{
            `properties`: #{
              `refillTokens`: #{}
            },
            `required`: #[
              "refillTokens"
            ]
          },
          #{
            `properties`: #{
              `refillIntervalMs`: #{}
            },
            `required`: #[
              "refillIntervalMs"
            ]
          }
        ]
      }
    }
  },
  #{
    `if`: #{
      `properties`: #{
        `consistency`: #{
          `const`: "strict"
        }
      },
      `required`: #[
        "consistency"
      ]
    },
    `then`: #{
      `properties`: #{
        `failureMode`: #{
          `const`: "fail-closed"
        },
        `coordinatorRequired`: #{
          `const`: true
        },
        `maximumOvershoot`: #{
          `const`: 0
        }
      }
    }
  },
  #{
    `if`: #{
      `properties`: #{
        `consistency`: #{
          `const`: "bounded"
        }
      },
      `required`: #[
        "consistency"
      ]
    },
    `then`: #{
      `properties`: #{
        `maximumOvershoot`: #{
          `type`: "integer",
          `minimum`: 1
        },
        `localDenyCacheEntries`: #{
          `type`: "integer",
          `minimum`: 1
        }
      }
    }
  },
  #{
    `if`: #{
      `properties`: #{
        `consistency`: #{
          `const`: "advisory"
        }
      },
      `required`: #[
        "consistency"
      ]
    },
    `then`: #{
      `properties`: #{
        `failureMode`: #{
          `enum`: #[
            "fail-open",
            "local-only"
          ]
        },
        `coordinatorRequired`: #{
          `const`: false
        }
      }
    }
  },
  #{
    `if`: #{
      `properties`: #{
        `operationClass`: #{
          `enum`: #[
            "auth-attempt",
            "auth-recovery",
            "mutation",
            "payment-or-ledger-write",
            "job-admission"
          ]
        }
      },
      `required`: #[
        "operationClass"
      ]
    },
    `then`: #{
      `properties`: #{
        `consistency`: #{
          `const`: "strict"
        },
        `failureMode`: #{
          `const`: "fail-closed"
        },
        `coordinatorRequired`: #{
          `const`: true
        },
        `maximumOvershoot`: #{
          `const`: 0
        }
      }
    }
  },
  #{
    `if`: #{
      `properties`: #{
        `layer`: #{
          `const`: "authorization"
        }
      },
      `required`: #[
        "layer"
      ]
    },
    `then`: #{
      `properties`: #{
        `consistency`: #{
          `const`: "strict"
        },
        `failureMode`: #{
          `const`: "fail-closed"
        },
        `coordinatorRequired`: #{
          `const`: true
        },
        `maximumOvershoot`: #{
          `const`: 0
        }
      }
    }
  },
  #{
    `if`: #{
      `allOf`: #[
        #{
          `properties`: #{
            `layer`: #{
              `const`: "cloudflare-edge"
            }
          },
          `required`: #[
            "layer"
          ]
        },
        #{
          `properties`: #{
            `enforcementMode`: #{
              `const`: "enforce"
            }
          },
          `required`: #[
            "enforcementMode"
          ]
        },
        #{
          `properties`: #{
            `operationClass`: #{
              `enum`: #[
                "auth-recovery",
                "mutation",
                "payment-or-ledger-write",
                "job-admission"
              ]
            }
          },
          `required`: #[
            "operationClass"
          ]
        }
      ]
    },
    `then`: false
  }
])
model RateLimitPolicyV2 {
  @pattern("^[A-Za-z0-9._-]{1,128}$")
  policyId: string;
  operationClass: OperationClass;
  algorithm: RateLimitAlgorithmV2;
  consistency: RateLimitConsistency;
  enforcementMode: RateLimitEnforcementMode;
  failureMode: RateLimitFailureMode;
  layer: RateLimitLayer;

  @minValue(1)
  @maxValue(1000000000)
  capacity: uint64;

  @minValue(1)
  @maxValue(2678400000)
  windowMs?: uint64;

  @minValue(1)
  @maxValue(1000000000)
  refillTokens?: uint64;

  @minValue(1)
  @maxValue(2678400000)
  refillIntervalMs?: uint64;

  @minValue(0)
  @maxValue(1000000000)
  maximumOvershoot: uint64;

  @minValue(0)
  @maxValue(10000)
  localDenyCacheEntries: uint32;

  coordinatorRequired: boolean;

  @pattern("^[A-Za-z0-9._-]{1,128}$")
  keyVersion: string;
}
