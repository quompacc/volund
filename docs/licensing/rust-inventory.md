# Rust-Abhängigkeitsinventar

Automatisch aus `cargo metadata --locked --offline` und den gegen Cargo.lock
SHA-256-geprüften `.crate`-Archiven erzeugt. Stand: 2026-09-08.
Alle 236 externen Pakete, einschließlich optionaler und plattformfremder Zweige.
Lizenzausdrücke sind unveränderte Paketangaben; `/` bezeichnet hier die ältere
Cargo-Schreibweise für eine Wahl. Volltexte: [rust-notices.txt](rust-notices.txt).
Die Text-IDs sind SHA-256 über UTF-8 mit LF. Sonderfall `wasite`: [Bewertung](README.md).

| Paket | Version | Lizenzangabe | Archiv-SHA-256 | Text-IDs (Präfixe) |
| --- | --- | --- | --- | --- |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 | 683d7910e743518b0e34f1186f92494becacb047c7b6bf616c96772180fef923 | 62c7a1e35f564068, 23f18e03dc49df91 |
| android_system_properties | 0.1.6 | MIT OR Apache-2.0 | ae221649c9976a6f6c56ae1facf410f3ddb33cc661c4b7b61020a912d4237fbc | 216486f29671a426, 80f275e90d799911 |
| arbitrary | 1.4.2 | MIT OR Apache-2.0 | c3d036a3c4ab069c7b410a2ce876bd74808d2d0888a82667669f8e783a898bf1 | a60eea8175145316, 15656cc11a8331f2 |
| argon2 | 0.5.3 | MIT OR Apache-2.0 | 3c3610892ee6e0cbce8ae2700349fcf8f98adb0dbfbee85aec3c9179d29cc072 | a9040321c3712d8f, 33f702959c0ea91c |
| atoi | 2.0.0 | MIT | f28d99ec8bfea296261ca1af174f24225171fea9664ba9003cbebee704810528 | afb11426e09da40a |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT | 1505bd5d3d116872e7271a6d4e16d81d0c8570876c8de68093a09ac269d8aac0 | a60eea8175145316, 23f18e03dc49df91, 6226d0632e2e1a80 |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | f2032f911046de80f0a198e0901378627c33f59ea0ac00e363d481118bd70a53 | a60eea8175145316, 27995d58ad5c1145 |
| axum-core | 0.5.6 | MIT | 08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1 | 008c87afcd2e626e |
| axum | 0.8.9 | MIT | 31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90 | 6a13bc24a100a681 |
| base64 | 0.22.1 | MIT OR Apache-2.0 | 72b3254f16251a8381aa12e40e3c4d2f0199f8c6508fbecb9d91f575e0fbb8c6 | a60eea8175145316, 0dd882e53de11566 |
| base64ct | 1.8.3 | Apache-2.0 OR MIT | 2af50177e190e07a26ab74f8b1efbfe2ef87da2116221318cb1c2e82baf7de06 | a9040321c3712d8f, 2d1c57bff28344b9 |
| bitflags | 2.13.1 | MIT OR Apache-2.0 | b588b76d00fde79687d7646a9b5bdf3cc0f655e0bbd080335a95d7e96f3587da | a60eea8175145316, 6485b8ed310d3f03 |
| blake2 | 0.10.6 | MIT OR Apache-2.0 | 46502ad458c9a52b69d4d4d32775c788b7a1b85e8bc9d482d92250fc0e3f8efe | a9040321c3712d8f, 9c768944eb4a0422 |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | 3078c7629b62d3f0439517fa394996acacc5cbc91c5a20d8c658e77abd503a71 | a9040321c3712d8f, d5c22aa3118d240e |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | 72f5acc6cb2ba439de613abc23857ec3d78374d8ed5ac84e9d11336e87da8649 | a60eea8175145316, 65f94e99ddaf4f5d |
| byteorder | 1.5.0 | Unlicense OR MIT | 1fd0f2584146f6f2ef48085050886acf353beff7305ebd1ae69500e27c67f64b | 01c266bced4a434d, 0f96a83840e146e4 |
| bytes | 1.12.1 | MIT | fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04 | 45f522cacecb1023 |
| cc | 1.4.4 | MIT OR Apache-2.0 | 0ad534f4357a5264cce5019c989cf66a4f0dc4e0d1b1d15f8aacec0ff7360273 | a60eea8175145316, 378f5840b258e277 |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 | 9330f8b2ff13f34540b44e946ef35111825727b38d33286ef986142615121801 | a60eea8175145316, 378f5840b258e277 |
| chrono-tz | 0.10.4 | MIT OR Apache-2.0 | a6139a8597ed92cf816dfb33f5dd6cf0bb93a6adc938f11039f371bc5bcd26c3 | 4789210f4df8abc0, 0613408568889f57 |
| chrono | 0.4.45 | MIT OR Apache-2.0 | 1aa79e62e7697b8e29b513a68abacf485adcd1fe8284a4316c5ae868e6633327 | 946c9835d8034d24 |
| const-oid | 0.9.6 | Apache-2.0 OR MIT | c2459377285ad874054d797f3ccebf984978aa39129f6eafde5cdc8315b612f8 | a9040321c3712d8f, bada9e7ed8dc00d6 |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | 773648b94d0e5d620f64f280777445740e61fe701025087ec8b57f45c791888b | a60eea8175145316, 62065228e42caebc |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | 59ed5838eebb26a2bb2e58f6d5b5316989ae9d08bab10e0e6d103e656d1b0280 | a9040321c3712d8f, ae9baa7beea91027 |
| crc-catalog | 2.5.0 | MIT OR Apache-2.0 | 217698eaf96b4a3f0bc4f3662aaa55bdf913cd54d7204591faa790070c6d0853 | d3cdb764b98283ee, 5ef8fcfb6cccec8f |
| crc | 3.4.0 | MIT OR Apache-2.0 | 5eb8a2a1cd12ab0d987a5d5e825195d372001a4094a0376319d5a0ad71c1ba0d | 470355a7eed93fcc, 3488679340a49ecc |
| crc32fast | 1.5.1 | MIT OR Apache-2.0 | 8498c871161e1742aaa9d52551b2d6ebdd4c3d45a3be423e3728f33b955be550 | c6596eb7be8581c1, 61d383b05b87d78f |
| crossbeam-queue | 0.3.13 | MIT OR Apache-2.0 | 803d13fb3b09d88be9f4dbc29062c66b19bf7170867ceb746d2a8689bf6c7a26 | a60eea8175145316, 5734ed989dfca1f6 |
| crossbeam-utils | 0.8.22 | MIT OR Apache-2.0 | 61803da095bee82a81bb1a452ecc25d3b2f1416d1897eb86430c6159ef717c17 | a60eea8175145316, 5734ed989dfca1f6 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | 78c8292055d1c1df0cce5d180393dc8cce0abec0a7102adb6c7b1eef6016d60a | a9040321c3712d8f, 3521672491a34794 |
| der | 0.7.10 | Apache-2.0 OR MIT | e7c1832837b905bbfb5101e07cc24c8deddf52f93225eee6ead5f4d63d53ddcb | a9040321c3712d8f, ad64fcb9589f1627 |
| derive_arbitrary | 1.4.2 | MIT OR Apache-2.0 | 1e567bd82dcff979e4b03460c307b3cdc9e96fde3d73bed1496d2bc75d9dd62a | a60eea8175145316, 15656cc11a8331f2 |
| digest | 0.10.7 | MIT OR Apache-2.0 | 9ed9a281f7bc9b7576e61468ba615a66a5c8cfdff42420a70aa82701a3b1e292 | a9040321c3712d8f, 9e0dfd2dd4173a53 |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 | c6232dd377dcc64799954cbd3a9bb882e9cdc1308ccd87b1c098f1fb2eaf82a8 | a60eea8175145316, 23f18e03dc49df91 |
| dotenvy | 0.15.7 | MIT | 1aaf95b3e5c8f23aa320147307562d361db0ae0d51242340f558153b4eb2439b | 37653ea50ac84bb5 |
| either | 1.18.0 | MIT OR Apache-2.0 | 252afb9ae5eaa683babdc6a068b3f5726eb19e05070c731f9b2a23a7c3e8ed34 | a60eea8175145316, 7576269ea71f767b |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | 877a4ace8713b0bcf2a4e7eec82529c029f1d0619886d18145fea96c3ffe5c0f | a60eea8175145316, 7365cc8878a1d7ce |
| errno | 0.3.14 | MIT OR Apache-2.0 | 39cab71617ae0d63f51a36d69f866391735b51691dbda63cf6f96d042b63efeb | a60eea8175145316, 8764a597675778dd |
| etcetera | 0.8.0 | MIT OR Apache-2.0 | 136d1b5283a1ab77bd9257427ffd09d8667ced0570b6f938942bc7568ed5b943 | 62c7a1e35f564068, 23f18e03dc49df91 |
| event-listener | 5.4.2 | Apache-2.0 OR MIT | 5a23add41df1562121a9393cb065eab5146a1242410f23a644851e90cfd669d2 | a60eea8175145316, 23f18e03dc49df91 |
| find-msvc-tools | 0.1.11 | MIT OR Apache-2.0 | d45db016d36b838f563236e9193d0ee6ce38f3f68b6c94e914b4929c96bbb890 | a60eea8175145316, 378f5840b258e277 |
| flate2 | 1.1.10 | MIT OR Apache-2.0 | 6e634e2e0ebac1ee034020da1ca582e17ffe4e0f5e985823721e168928136dcb | a60eea8175145316, 025436edff4cfcdd |
| flume | 0.11.1 | Apache-2.0/MIT | da0e4dd2a88388a1f4ccc7c9ce104604dab68d9f408dc34cd45823d5a9069095 | d8621ec2eee5b9ca, 30fefc3a7d6a0041 |
| foldhash | 0.1.5 | Zlib | d9c4f5dac5e15c24eb999c26181a6ca40b39fe946cbe4c263c7209467bc83af2 | b1181a40b2a7b25c |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | cb4cb245038516f5f85277875cdaa4f7d2c9a0fa0468de06ed190163b1581fcf | a60eea8175145316, 20c7855c364d57ea |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 | b1f9e3d69d39e4862ffed03ed071a76f9a13ba1d9109d355b0f0aa6b15e393c4 | 275c491d6d116055, 6652c868f35dfe5e |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | 92d699e522242e69e3003b94ecc1f960f3a5e015aa7c5d7486e65ad01dd94f5e | 275c491d6d116055, 6652c868f35dfe5e |
| futures-executor | 0.3.34 | MIT OR Apache-2.0 | 031b47cf1a3c6cc8bc2fc76cd437f521619387907d469316e7c0bc278f1f5432 | 275c491d6d116055, 6652c868f35dfe5e |
| futures-intrusive | 0.5.0 | MIT OR Apache-2.0 | 1d930c203dd0b6ff06e0201a4a2fe9149b43c684fd4420555b26d21b1a02956f | 4618209e13998293, 161c4bcb09e94e97 |
| futures-io | 0.3.34 | MIT OR Apache-2.0 | 53c0fa8157de1303bfffdaa1cc2a673bfffb60102f76b0ef4441659124373fed | 275c491d6d116055, 6652c868f35dfe5e |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | 1944426bf7d03f1d14f708785e4b33efd750b36d48a157b836b3efc15ede8e1d | 275c491d6d116055, 6652c868f35dfe5e |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | cd417de3d1d015fc3bfd2b1ea46dfc7bab72ef86f1cc7cc9c78e728b34a6d1fd | 275c491d6d116055, 6652c868f35dfe5e |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | 0d50a92467f8ba5dd6e3ee5d4bd04d73ab2e4e1c44474a0674821dfce14b79bc | 275c491d6d116055, 6652c868f35dfe5e |
| generic-array | 0.14.7 | MIT | 85649ca51fd72272d7821adaf274ad91c288277713d9c18820d8499a7ff69e9a | ad4fcfaf8d5b12b9 |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | ff2abc00be7fca6ebc474524697ae276ad847ad0a6b3faa4bcb027e9a4614ad0 | aaff376532ea30a0, 42fa16951ce7f24b |
| hashbrown | 0.15.5 | MIT OR Apache-2.0 | 9229cfe53dfd69f0609a49f65461bd93001ea1ef889cd5529dd176593f5338a1 | a60eea8175145316, ff8f68cb076caf8c |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | ed5909b6e89a2db4456e54cd5f673791d7eca6732202bbf2a9cc504fe2f9b84a | a60eea8175145316, ff8f68cb076caf8c |
| hashlink | 0.10.0 | MIT OR Apache-2.0 | 7382cf6263419f2d8df38c55d7da83da5c18aef87fc7a7fc1fb1e344edfe14c1 | c144680885b29e47, e915669a595b11a2 |
| heck | 0.5.0 | MIT OR Apache-2.0 | 2304e00983f87ffb38b55b444b5e3b60a884b5d30c0fca7d82fe33449bbe55ea | a60eea8175145316, 7b63ecd5f1902af1 |
| hex | 0.4.3 | MIT OR Apache-2.0 | 7f24254aa9a54b5c858eaee2f5bccdb46aaf0e486a595ed5fd8f86ba55232a70 | c6596eb7be8581c1, f7bdb3426d045cd5 |
| hkdf | 0.12.4 | MIT OR Apache-2.0 | 7b5f8eb2ad728638ea2c7d47a21db23b7b58a72ed6a38256b8a1849f15fbbdf7 | 59013a5c8d3a19c2, d288f9c9b4590446 |
| hmac | 0.12.1 | MIT OR Apache-2.0 | 6c49c37c09c17a53d937dfbb742eb3a961d65a994e6bcdcf37e7399d0cc8ab5e | a9040321c3712d8f, 9e0dfd2dd4173a53 |
| home | 0.5.11 | MIT OR Apache-2.0 | 589533453244b0995c858700322199b2becb13b627df2851f64a2775d024abcf | 8ada45cd9f843acf, 23f18e03dc49df91 |
| http-body-util | 0.1.5 | MIT | 23169fe34a5fbcdd3f3862e78fb9b6fccd5f02a6dc6f732547005d45631ce71c | 248378d0a3383c17 |
| http-body | 1.1.0 | MIT | ca2a8f2913ee65f60facd6a5905613afaa448497a0230cc41ce022d93290bc2c | 248378d0a3383c17 |
| http-range-header | 0.4.2 | MIT | 9171a2ea8a68358193d15dd5d70c1c10a2afc3e7e4c5bc92bc9f025cebd7359c | ed72515b08d7ce8a |
| http | 1.5.0 | MIT OR Apache-2.0 | 918d3568bebf352712bc2ef3d46a8bcf1a75b373be6539de198e9105cbbf9ce0 | 8bb1b50b0e5c9399, dc91f8200e4b2a1f |
| httparse | 1.10.1 | MIT OR Apache-2.0 | 6dbf3de79e51f3d586ab4cb9d5c3e2c14aa28ed23d180cf89b4df0454a69cc87 | a60eea8175145316, 391a5396cec6230b |
| httpdate | 1.0.3 | MIT OR Apache-2.0 | df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9 | 4d10fe5f3aa176b0, 934887691e05d69d |
| hyper-util | 0.1.20 | MIT | 96547c2556ec9d12fb1578c4eaf448b04993e7fb79cbaad930a656880a6bdfa0 | 9e0a97848ea543ae |
| hyper | 1.11.1 | MIT | 27b501faa50e7a26c3d3560ca625132f4078a17771f4810baf70475ae48cbe43 | 2d01890414494742 |
| iana-time-zone-haiku | 0.1.2 | MIT OR Apache-2.0 | f31827a206f56af32e590ba56d5d2d085f558508192593743f16b2306495269f | 696759d65dfe558f, da28ccc6b158fc2d |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 | e31bc9ad994ba00e440a8aa5c9ef0ec67d5cb5e5cb0cc7f8b744a35b389cc470 | 696759d65dfe558f, da28ccc6b158fc2d |
| icu_collections | 2.1.1 | Unicode-3.0 | 4c6b649701667bbe825c3b7e6388cb521c23d88644678e83c0c4d0a621a34b43 | f367c1b8e1aa2624 |
| icu_locale_core | 2.1.1 | Unicode-3.0 | edba7861004dd3714265b4db54a3c390e880ab658fec5f7db895fae2046b5bb6 | f367c1b8e1aa2624 |
| icu_normalizer_data | 2.1.1 | Unicode-3.0 | 7aedcccd01fc5fe81e6b489c15b247b8b0690feb23304303a9e560f37efc560a | f367c1b8e1aa2624 |
| icu_normalizer | 2.1.1 | Unicode-3.0 | 5f6c8828b67bf8908d82127b2054ea1b4427ff0230ee9141c54251934ab1b599 | f367c1b8e1aa2624 |
| icu_properties_data | 2.1.2 | Unicode-3.0 | 616c294cf8d725c6afcd8f55abc17c56464ef6211f9ed59cccffe534129c77af | f367c1b8e1aa2624 |
| icu_properties | 2.1.2 | Unicode-3.0 | 020bfc02fe870ec3a66d93e677ccca0562506e5872c650f893269e08615d74ec | f367c1b8e1aa2624 |
| icu_provider | 2.1.1 | Unicode-3.0 | 85962cf0ce02e1e0a629cc34e7ca3e373ce20dda4c4d7294bbd0bf1fdb59e614 | f367c1b8e1aa2624 |
| idna_adapter | 1.2.1 | Apache-2.0 OR MIT | 3acae9609540aa318d1bc588455225fb2085b9ed0c4f6bd0d9d5bcd86f1a0344 | a60eea8175145316, 8b43ce8accd61e9d |
| idna | 1.1.0 | MIT OR Apache-2.0 | 3b0875f23caa03898994f6ddc501886a45c7d3d62d04d2d90788d47be1b1e4de | a60eea8175145316, b38f11f6096706e6 |
| indexmap | 2.14.0 | Apache-2.0 OR MIT | d466e9454f08e4a911e14806c24e16fba1b4c121d1ea474396f396069cf949d9 | a60eea8175145316, ecc269ef87fd38a1 |
| itoa | 1.0.18 | MIT OR Apache-2.0 | 8f42a60cbdf9a97f5d2305f08a87dc4e09308d1276d28c869c684d7777685682 | 62c7a1e35f564068, 23f18e03dc49df91 |
| js-sys | 0.3.104 | MIT OR Apache-2.0 | 0e0c1080212aad755ea003d18543e8768dd432c48819efd73a7bf1e39b7a5a3a | a60eea8175145316, 378f5840b258e277 |
| lazy_static | 1.5.0 | MIT OR Apache-2.0 | bbd2bcb4c963f2ddae06a2efc7e9f3591312473c50c6685e1f298068316e66fe | a60eea8175145316, 0621878e61f0d0fd |
| libc | 0.2.189 | MIT OR Apache-2.0 | 3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2 | 62c7a1e35f564068, 123a331b5dbf04c3 |
| libm | 0.2.16 | MIT | b6d2cec3eae94f9f509c767b45932f1ada8350c4bdb85af2fcab4a3c14807981 | 3823dda7cf046602 |
| libredox | 0.1.21 | MIT | d7955dfc218a8afb29dfeffd540e3a6e96baeb94fe7138228dd7cc6937fbbf96 | 8d073a6a80d1ef2d |
| libsqlite3-sys | 0.30.1 | MIT | 2e99fb7a497b1e3339bc746195567ed8d3e24945ecd636e3619d20b9de9e9149 | f59ba65550f2a5ad, ea4fcb309f14a220 |
| litemap | 0.8.3 | Unicode-3.0 | 47d9d19d1d6efa0109d2f65ff4c85cddd50bd572e5a00127ab10987290bcefae | f367c1b8e1aa2624 |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | 224399e74b87b5f3557511d98dff8b14089b3dadafcab6bb93eab67d3aace965 | a60eea8175145316, c9a75f18b9ab2927 |
| log | 0.4.34 | MIT OR Apache-2.0 | f9f8bd3e56ce4dfc153cf470fffbfa98c7620958b312ca5c3a4b8d5181fd13c6 | a60eea8175145316, 6485b8ed310d3f03 |
| matchit | 0.8.4 | MIT AND BSD-3-Clause | 47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3 | de701d0618d694fe, 162ce11ad71338d0 |
| md-5 | 0.10.6 | MIT OR Apache-2.0 | d89e7ee0cfbedfc4da3340218492196241d89eefb6dab27de5df917a6d2e78cf | a9040321c3712d8f, b4eb00df6e2a4d22 |
| memchr | 2.8.3 | Unlicense OR MIT | cf8baf1c55e62ffcace7a9f06f4bd9cd3f0c4beb022d3b367256b91b87513d98 | 01c266bced4a434d, 0f96a83840e146e4 |
| mime_guess | 2.0.5 | MIT | f7c44f8e672c00fe5308fa235f821cb4198414e1c77935c1ab6948d3fd78550e | 6919f1acec82afc7 |
| mime | 0.3.17 | MIT OR Apache-2.0 | 6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a | a60eea8175145316, df9cfd06d8a44d9a |
| mio | 1.2.2 | MIT | 30d65c71f1ce40ab09135ce117d742b9f8a19ff91a41a8b57ed50bc2de59c427 | 07919255c7e04793 |
| num-bigint-dig | 0.8.6 | MIT/Apache-2.0 | e661dda6640fad38e827a6d4a310ff4763082116fe217f279885c97f511bb0b7 | a60eea8175145316, 6485b8ed310d3f03 |
| num-integer | 0.1.47 | MIT OR Apache-2.0 | 7ce2d95d4b3734dc35aa2f45e1aa22cd416814592a4f9d9205e11affd5b8e10b | a60eea8175145316, 6485b8ed310d3f03 |
| num-iter | 0.1.46 | MIT OR Apache-2.0 | c92800bd69a1eac91786bcfe9da64a897eb72911b8dc3095decbd07429e8048b | a60eea8175145316, 6485b8ed310d3f03 |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | 071dfc062690e90b734c0b2273ce72ad0ffa95f0c74596bc250dcfd960262841 | a60eea8175145316, 6485b8ed310d3f03 |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | 9f7c3e4beb33f85d45ae3e3a1792185706c8e16d043238c593331cc7cd313b50 | a60eea8175145316, 23f18e03dc49df91 |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | 2621685985a2ebf1c516881c026032ac7deafcda1a2c9b7850dc81e3dfcb64c1 | a60eea8175145316, c9a75f18b9ab2927 |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | 93857453250e3077bd71ff98b6a65ea6621a19bb0f559a85248955ac12c45a1a | a60eea8175145316, c9a75f18b9ab2927 |
| parking | 2.2.1 | Apache-2.0 OR MIT | f38d5652c16fde515bb1ecef450ab0f6a219d619a7274976324d5e377f7dceba | a60eea8175145316, 23f18e03dc49df91, 9a6d7a3c1b8edcc7 |
| password-hash | 0.5.0 | MIT OR Apache-2.0 | 346f04948ba92c43e8469c1ee6736c7563d71012b17d40745260fe106aac2166 | a9040321c3712d8f, 233b95ccbf90dc67 |
| pem-rfc7468 | 0.7.0 | Apache-2.0 OR MIT | 88b39c9bfcfc231068454382784bb460aae594343fb030d46e9f50a645418412 | a9040321c3712d8f, 90c503b61dee04e1 |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | 9b4f627cb1b25917193a259e49bdad08f671f8d9708acfd5fe0a8c1455d87220 | a60eea8175145316, b38f11f6096706e6 |
| phf_shared | 0.12.1 | MIT | 06005508882fb681fd97892ecff4b7fd0fee13ef1aa569f8695dae7ab9099981 | 0ab4d106b6faac07 |
| phf | 0.12.1 | MIT | 913273894cec178f401a31ec4b656318d95473527be05c0752cc41cdc32be8b7 | 0ab4d106b6faac07 |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | a89322df9ebe1c1578d689c92318e070967d1042b512afbe49518723f4e6d5cd | 0d542e0c8804e39a, 23f18e03dc49df91 |
| pkcs1 | 0.7.5 | Apache-2.0 OR MIT | c8ffb9f10fa047879315e6625af03c164b16962a5368d724ed16323b68ace47f | a9040321c3712d8f, c995204cc6bad2ed |
| pkcs8 | 0.10.2 | Apache-2.0 OR MIT | f950b2377845cebe5cf8b5165cb3cc1a5e0fa5cfa3e1f7f55707d8fd82e0a7b7 | a9040321c3712d8f, ad64fcb9589f1627 |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 | f6b464fbc74e149a392436b17d523f769e057cb6877f6a5c4618bc6f11800548 | a60eea8175145316, 378f5840b258e277 |
| plain | 0.2.3 | MIT/Apache-2.0 | b4596b6d070b27117e987119b4dac604f3c58cfb0b191112e24771b2faeac1a6 | a60eea8175145316, bc12b75fd8182981 |
| potential_utf | 0.1.6 | Unicode-3.0 | d83eb9bc6d8e5cf568e7a1101d60ee05e81ed50ea106026f3d18deeb046d7661 | f367c1b8e1aa2624 |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 | 85eae3c4ed2f50dcfe72643da4befc30deadb458a9b590d720cde2f2b1e97da9 | 0218327e7a480793, 4cada0bd02ea3692 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | 985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9 | 62c7a1e35f564068, 23f18e03dc49df91 |
| quote | 1.0.47 | MIT OR Apache-2.0 | 1fbf4db142a473a8d80c26bbf18454ed458bf8d26c8219c331daecfdbd079001 | 62c7a1e35f564068, 23f18e03dc49df91 |
| rand_chacha | 0.3.1 | MIT OR Apache-2.0 | e6c10a63a0fa32252be49d21e7709d4d4baf8d231c2dbce1eaa8141b9b127d88 | 90eb64f0279b0d94, aaff376532ea30a0, 209fbbe0ad52d923 |
| rand_core | 0.6.4 | MIT OR Apache-2.0 | ec0be4795e2f6a28069bec0b5ff3e2ac9bafc99e6a9a7dc3547996c5c816922c | 90eb64f0279b0d94, 6df43f6f4b5d4587, 209fbbe0ad52d923 |
| rand | 0.8.8 | MIT OR Apache-2.0 | e058c7de0b26af77780c769414d6257830bb240f3c38477dbc2c16e5f54d6d4c | 90eb64f0279b0d94, 35242e7a83f69875, 209fbbe0ad52d923 |
| redox_syscall | 0.5.18 | MIT | ed2bf2547551a7053d6fdfafda3f938979645c44812fbfcda098faae3f1a362d | efcfee7981ff7243 |
| redox_syscall | 0.9.3 | MIT | d678d17679829e73d371e96880897e98fee2ded7acc0a50bdf8af2affa4b2fe5 | efcfee7981ff7243 |
| rsa | 0.9.10 | MIT OR Apache-2.0 | b8573f03f5883dcaebdfcf4725caa1ecb9c15b2ef50c43a07b816e06799bb12d | 769f80b5bcb42ed0, 30fefc3a7d6a0041 |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | cf54715a573b99ac80df0bc206da022bcd442c974952c7b9720069370852e21f | 62c7a1e35f564068, 23f18e03dc49df91 |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | 9774ba4a74de5f7b1c1451ed6cd5285a32eddb5cccb8cc655a4e50009e06477f | 62c7a1e35f564068, c9bff75738922193 |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | 94143f37725109f92c262ed2cf5e59bce7498c01bcc1502d7b9afe439a4e9f49 | a60eea8175145316, fb77f0a9c53e473a |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | 67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48 | 62c7a1e35f564068, 23f18e03dc49df91 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348 | 62c7a1e35f564068, 23f18e03dc49df91 |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14 | 62c7a1e35f564068, 23f18e03dc49df91 |
| serde_path_to_error | 0.1.20 | MIT OR Apache-2.0 | 10a9ff822e371bb5403e391ecd83e182e0e77ba7f6fe0160b795797109d1b457 | 62c7a1e35f564068, 23f18e03dc49df91 |
| serde_urlencoded | 0.7.1 | MIT/Apache-2.0 | d3491c14715ca2294c4d6a88f15e84739788c1d030eed8c110436aafdaa2f3fd | 62c7a1e35f564068, b9eb266294324f67 |
| serde | 1.0.229 | MIT OR Apache-2.0 | 4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba | 62c7a1e35f564068, 23f18e03dc49df91 |
| sha1 | 0.10.7 | MIT OR Apache-2.0 | a978451301f4db1d02937a4ab3ccce137717b81826e79b7d49ffe3244a13c3b8 | a9040321c3712d8f, b4eb00df6e2a4d22 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | a7507d819769d01a365ab707794a4084392c824f54a7a6a7862f8c3d0892b283 | a9040321c3712d8f, b4eb00df6e2a4d22 |
| shlex | 2.0.1 | MIT OR Apache-2.0 | f8fadd59c855ef2080decdef8ff161eb6661b86933c9d82e5ba29dc602a55aba | 553fffcd9b1cb158, 4455bf75a9115410 |
| signal-hook-registry | 1.4.8 | MIT OR Apache-2.0 | c4db69cba1110affc0e9f7bcd48bbf87b3f4fc7c61fc9155afd4c469eb3d6c1b | a60eea8175145316, 503558bfefe66ca1 |
| signature | 2.2.0 | Apache-2.0 OR MIT | 77549399552de45a898a580c1b41d445bf730df867cc44e6c0233bbc4b8329de | a9040321c3712d8f, b3470648aff02beb |
| simd-adler32 | 0.3.10 | MIT | 3a219298ac11a56ea9a6d2120044824d6f01aeb034955e7af7bc16858527deea | 42a35170233e83e1 |
| siphasher | 1.0.3 | MIT/Apache-2.0 | 8ee5873ec9cce0195efcb7a4e9507a04cd49aec9c83d0389df45b1ef7ba2e649 | c962ee4d1d05ddc1 |
| slab | 0.4.12 | MIT | 0c790de23124f9ab44544d7ac05d60440adc586479ce501c1d6d7da3cd8c9cf5 | 8ce0830173fdac60 |
| smallvec | 1.15.2 | MIT OR Apache-2.0 | 8ed6a63f02c8539c91a8685a86f4099661ba3da017932f6ebbea6de3f0fa7c90 | a60eea8175145316, 0b28172679e0009b |
| socket2 | 0.6.5 | MIT OR Apache-2.0 | c3d1e2c7f27f8d4cb10542a02c49005dbd6e93095799d6f3be745fae9f8fedd4 | a60eea8175145316, 378f5840b258e277 |
| spin | 0.9.9 | MIT | 3763264f6b73151db08c50ff20d7d8a0b8796e021cdea7ceedad07b80155fa0e | 6ac8711fb340c62c |
| spki | 0.7.3 | Apache-2.0 OR MIT | d91ed6c858b01f942cd56b37a94b3e0a1798290327d1236e4d9cf4eaca44d29d | a9040321c3712d8f, c995204cc6bad2ed |
| sqlx-core | 0.8.6 | MIT OR Apache-2.0 | ee6798b1838b6a0f69c007c133b8df5866302197e404e8b6ee8ed3e3a5e68dc6 | 9be39331309bc958, 44b1600a26039a75 |
| sqlx-macros-core | 0.8.6 | MIT OR Apache-2.0 | 19a9c1841124ac5a61741f96e1d9e2ec77424bf323962dd894bdb93f37d5219b | 9be39331309bc958, 44b1600a26039a75 |
| sqlx-macros | 0.8.6 | MIT OR Apache-2.0 | a2d452988ccaacfbf5e0bdbc348fb91d7c8af5bee192173ac3636b5fb6e6715d | 9be39331309bc958, 44b1600a26039a75 |
| sqlx-mysql | 0.8.6 | MIT OR Apache-2.0 | aa003f0038df784eb8fecbbac13affe3da23b45194bd57dba231c8f48199c526 | 9be39331309bc958, 44b1600a26039a75 |
| sqlx-postgres | 0.8.6 | MIT OR Apache-2.0 | db58fcd5a53cf07c184b154801ff91347e4c30d17a3562a635ff028ad5deda46 | 9be39331309bc958, 44b1600a26039a75 |
| sqlx-sqlite | 0.8.6 | MIT OR Apache-2.0 | c2d12fe70b2c1b4401038055f90f151b78208de1f9f89a7dbfd41587a10c3eea | 9be39331309bc958, 44b1600a26039a75 |
| sqlx | 0.8.6 | MIT OR Apache-2.0 | 1fefb893899429669dcdd979aff487bd78f4064e5e7907e4269081e0ef7d97dc | 9be39331309bc958, 44b1600a26039a75 |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | 6ce2be8dc25455e1f91df71bfa12ad37d7af1092ae736f3a6cd0e37bc7810596 | a60eea8175145316, 5e05b024f653a5ce |
| stringprep | 0.1.5 | MIT/Apache-2.0 | 7b4df3d392d81bd458a8a621b8bffbd2302a12ffe288a9d931670948749463b1 | c6596eb7be8581c1, a07450fd4496cb8b |
| subtle | 2.6.1 | BSD-3-Clause | 13c2bddecc57b384dee18652358fb23172facb8a2c51ccc10d74c157bdea3292 | d1fc1bc0d155df60 |
| syn | 2.0.119 | MIT OR Apache-2.0 | 872831b642d1a07999a962a351ed35b955ea2cfc8f3862091e2a240a84f17297 | 62c7a1e35f564068, 23f18e03dc49df91 |
| syn | 3.0.4 | MIT OR Apache-2.0 | e6275cddf4610d1775e6d1fe9469b2e77d0f39fd98fb7450901b821e0c53649f | 62c7a1e35f564068, 23f18e03dc49df91 |
| sync_wrapper | 1.0.2 | Apache-2.0 | 0bf256ce5efdfa370213c1dabab5935a12e49f2c58d15e9eac2870d3b4f27263 | 0d542e0c8804e39a |
| synstructure | 0.13.2 | MIT | 728a70f3dbaf5bab7f0c4b1ac8d7ae5ea60a4b5549c8a5914361c99147a709d2 | 219920e865eee70b |
| thiserror-impl | 2.0.20 | MIT OR Apache-2.0 | bc04cd3e1236dd4a98afca4569f2deb3f120e5422a4023be2cb683f8486292af | 62c7a1e35f564068, 23f18e03dc49df91 |
| thiserror | 2.0.20 | MIT OR Apache-2.0 | ec86235f5fcc2a73650310756d2ac5b138a5780bbbdfae3eeccec992c435ba4f | 62c7a1e35f564068, 23f18e03dc49df91 |
| tinystr | 0.8.4 | Unicode-3.0 | b1e27c91459209c2986af3dcf603a5a74a4368754ce37414f59acc971167f643 | f367c1b8e1aa2624 |
| tinyvec_macros | 0.1.1 | MIT OR Apache-2.0 OR Zlib | 1f3ccbac311fea05f86f61904b462b55fb3df8837a366dfc601a0161d0532f20 | 4f44572785f35152, 1dd8eca0f83669e7, 41ace205715d9f19 |
| tinyvec | 1.12.0 | Zlib OR Apache-2.0 OR MIT | bb4ebadaa0af04fab11ae01eb5f9fdb5f9c5b875506e210e71c07873528baa7f | cfc7749b96f63bd3, fd80a26fbb3f644a, 84b34dd7608f7fb9 |
| tokio-macros | 2.7.2 | MIT | 78773a2a397f451582ce068015985c33193cf6dea8b74d2a639fe457b2f07b0e | 0b83dc40cba89b99 |
| tokio-stream | 0.1.19 | MIT | a3d06f0b082ba57c26b79407372e57cf2a1e28124f78e9479fe80322cf53420b | 253cd04c6714889d |
| tokio-util | 0.7.19 | MIT | 494815d09bf52b5548659851081238f0ca39ff638363907596da739561c62c52 | 253cd04c6714889d |
| tokio | 1.53.1 | MIT | 202caea871b69668250d242070849eb495be178ed697a3e98aebce5bc81a0bed | 253cd04c6714889d |
| tower-http | 0.6.11 | MIT | 4cfcf7e2740e6fc6d4d688b4ef00650406bb94adf4731e43c096c3a19fe40840 | 5049cf464977eff4 |
| tower-layer | 0.3.3 | MIT | 121c2a6cda46980bb0fcd1647ffaf6cd3fc79a013de288782836f6df9c48780e | 4249c8e6c5ebb85f |
| tower-service | 0.3.3 | MIT | 8df9b6e13f2d32c91b9bd719c00d1958837bc7dec474d94952798cc8e69eeec3 | 4249c8e6c5ebb85f |
| tower | 0.5.3 | MIT | ebe5ef63511595f1344e2d5cfa636d973292adc0eec1f0ad45fae9f0851ab1d4 | 4249c8e6c5ebb85f |
| tracing-attributes | 0.1.31 | MIT | 7490cfa5ec963746568740651ac6781f701c9c5ea257c58e057f3ba8cf69e8da | 898b1ae9821e98da |
| tracing-core | 0.1.36 | MIT | db97caf9d906fbde555dd62fa95ddba9eecfd14cb388e4f491a66d74cd5fb79a | 898b1ae9821e98da, 58545fed1565e42d |
| tracing | 0.1.44 | MIT | 63e71662fa4b2a2c3a26f570f037eb95bb1f85397f3cd8076caed2f026a6d100 | 898b1ae9821e98da |
| typenum | 1.20.1 | MIT OR Apache-2.0 | b6f5e870be6c3b371b77fe0ee0bafb859fa4964b4404c27de1d380043c4dda20 | db11fec9946737df, 516b24e051bf5630, a825bd853ab71619 |
| unicase | 2.9.0 | MIT OR Apache-2.0 | dbc4bc3a9f746d862c45cb89d705aa10f187bb96c76001afab07a0d35ce60142 | a60eea8175145316, 4f6bd11a0f17fe5b |
| unicode-bidi | 0.3.18 | MIT OR Apache-2.0 | 5c1cb5db39152898a79168971543b1cb5020dff7fe43c8dc468b0885f5e29df5 | edb20b474f6cbd4f, a60eea8175145316, 7b63ecd5f1902af1 |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 | e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75 | 62c7a1e35f564068, 23f18e03dc49df91, f7db81051789b729 |
| unicode-normalization | 0.1.25 | MIT OR Apache-2.0 | 5fd4f6878c9cb28d874b009da9e8d183b5abc80117c40bbd187a1fde336be6e8 | 23860c2a7b5d96b2, a60eea8175145316, 7b63ecd5f1902af1 |
| unicode-properties | 0.1.4 | MIT/Apache-2.0 | 7df058c713841ad818f1dc5d3fd88063241cc61f49f5fbea4b951e8cf5a8d71d | 23860c2a7b5d96b2, a60eea8175145316, 7b63ecd5f1902af1 |
| url | 2.5.8 | MIT OR Apache-2.0 | ff67a8a4397373c3ef660812acab3268222035010ab8680ec4215f38ba3d0eed | a60eea8175145316, b38f11f6096706e6 |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT | b6c140620e7ffbb22c2dee59cafe6084a59b5ffc27a8859a5f0d494b5d52b6be | c30152c94a6d75e0, cfc7749b96f63bd3, 3fa4ca83dcc92378 |
| vcpkg | 0.2.15 | MIT/Apache-2.0 | accd4ea62f7bb7a82fe23066fb0957d48ef677f6eeb8215f372f52e48bb32426 | 3708458dee7f359a, ff7d99434986ba68 |
| version_check | 0.9.5 | MIT/Apache-2.0 | 0b928f33d975fc6ad9f86c8f283853ad26bdd5b10b7f1542aa2fa15e2289105a | a60eea8175145316, b7e650f3fce5c532 |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | ccf3ec651a847eb01de73ccad15eb7d99f80485de043efb2f370cd654f4ea44b | a60eea8175145316, 268872b9816f90fd, 23f18e03dc49df91 |
| wasite | 0.1.0 | Apache-2.0 OR BSL-1.0 OR MIT | b8dad83b4f25e74f184f64c43b150b91efe7647395b42289f38e50566d82855b | BSL-1.0, siehe README |
| wasm-bindgen-macro-support | 0.2.127 | MIT OR Apache-2.0 | e11d33f857dc2fb11b8bc75aee111aa9cbeb12cd9f25efd3d4c2a3dd4e235284 | a60eea8175145316, 378f5840b258e277 |
| wasm-bindgen-macro | 0.2.127 | MIT OR Apache-2.0 | 77775f8f3f7217702089053b94958f8f54061a3f663417df76e19cbdcca29bc1 | a60eea8175145316, 378f5840b258e277 |
| wasm-bindgen-shared | 0.2.127 | MIT OR Apache-2.0 | 7ef64dbcc55df09c7e5a46182d181c2cfa3e925f3da937ea764728b4bbb9dcbf | a60eea8175145316, 378f5840b258e277 |
| wasm-bindgen | 0.2.127 | MIT OR Apache-2.0 | 1b70935747edd64d89de3efa29d73789b806c15798f8e7dca4d8ac356b50ce70 | a60eea8175145316, 378f5840b258e277 |
| whoami | 1.6.1 | Apache-2.0 OR BSL-1.0 OR MIT | 5d4a4db5077702ca3015d3d02d74974948aba2ad9e12ab7df718ee64ccd7e97d | 62c7a1e35f564068, c9bff75738922193, 508a77d2e7b51d98 |
| windows_aarch64_gnullvm | 0.48.5 | MIT OR Apache-2.0 | 2b38e32f0abccf9987a4e3079dfb67dcd799fb61361e53e2882c3cbaf0d905d8 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | 32a4622180e7a0ec044bb555404c800bc9fd9ec262ec147edd5989ccd0c02cd3 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_aarch64_msvc | 0.48.5 | MIT OR Apache-2.0 | dc35310971f3b2dbbf3f0690a219f40e2d9afcf64f9ab7cc1be722937c26b4bc | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | 09ec2a7bb152e2252b53fa7803150007879548bc709c039df7627cabbd05d469 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_i686_gnu | 0.48.5 | MIT OR Apache-2.0 | a75915e7def60c94dcef72200b9a8e58e5091744960da64ec734a6c6e9b3743e | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | 8e9b5ad5ab802e97eb8e295ac6720e509ee4c243f69d781394014ebfe8bbfa0b | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | 0eee52d38c090b3caa76c563b86c3a4bd71ef1a819287c19d586d7334ae8ed66 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_i686_msvc | 0.48.5 | MIT OR Apache-2.0 | 8f55c233f70c4b27f66c523580f78f1004e8b5a8b659e05a4eb49d4166cca406 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | 240948bc05c5e7c6dabba28bf89d89ffce3e303022809e73deaefe4f6ec56c66 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_x86_64_gnu | 0.48.5 | MIT OR Apache-2.0 | 53d40abd2583d23e4718fddf1ebec84dbff8381c07cae67ff7768bbf19c6718e | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | 147a5c80aabfbf0c7d901cb5895d1de30ef2907eb21fbbab29ca94c5b08b1a78 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_x86_64_gnullvm | 0.48.5 | MIT OR Apache-2.0 | 0b7b52767868a23d5bab768e390dc5f5c55825b6d30b86c844ff2dc7414044cc | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | 24d5b23dc417412679681396f2b49f3de8c1473deb516bd34410872eff51ed0d | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_x86_64_msvc | 0.48.5 | MIT OR Apache-2.0 | ed94fce61571a4006852b7389a063ab983c02eb1bb37b47f8272ce92d06d9538 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | 589f6da84c646204747d1270a2a5661ea66ed1cced2631d546fdfb155959f9ec | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | b8e83a14d34d0623b51dce9581199302a221863196a1dde71a7663a4c2be9deb | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | 053e2e040ab57b9dc951b72c264860db7eb3b0200ba345b4e4c3b14f67855ddf | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | 3f316c4a2570ba26bbec722032c4099d8c8bc095efccdc15688708623367e358 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | f0805222e57f7521d6a62e36fa9163bc891acd422f971defe97d64e70d0a4fe5 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | 7781fa89eaf60850ac3d2da7af8e5242a5ea78d1a11c49bf2910bb5a73853eb5 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | 7837d08f69c77cf6b07689544538e017c1bfcf57e34b4c0ff58e6c2cd3b37091 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows-sys | 0.48.0 | MIT OR Apache-2.0 | 677d2418bec65e3338edb076e806bc1ec15693c5d0104683f2efe857f61056a9 | c16f8dcf1a368b83, c2cfccb812fe4821, 83caf48e8509b0f3 |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 | 1e38bc4d79ed67fd075bcc251a1c39b32a1776bbe92e5bef1f0bf1f8c531853b | c16f8dcf1a368b83, c2cfccb812fe4821, e8b9248a246d6264 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | ae137229bcbd6cdf0f7b80a31df61766145077ddf49416a728b02cb3921ff3fc | c16f8dcf1a368b83, c2cfccb812fe4821, a48ff77be5dd144f |
| windows-targets | 0.48.5 | MIT OR Apache-2.0 | 9a2fa6e2155d7247be68c096456083145c183cbbbc2764150dda45a87197940c | c16f8dcf1a368b83, c2cfccb812fe4821 |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | 9b724f72796e036ab90c1021d4780d4d3d648aca59e491e6b98e725b84e99973 | c16f8dcf1a368b83, c2cfccb812fe4821 |
| writeable | 0.6.4 | Unicode-3.0 | 3ad82d2a33cdc9674dc7465672f271e096168fcdbe0f799d9e6db8c5892679dc | f367c1b8e1aa2624 |
| yoke-derive | 0.8.2 | Unicode-3.0 | de844c262c8848816172cef550288e7dc6c7b7814b4ee56b3e1553f275f1858e | f367c1b8e1aa2624 |
| yoke | 0.8.3 | Unicode-3.0 | 709fe23a0424b6a435d82152b1bd3fdfb0833487d5fa90d05d42762a9891fef5 | f367c1b8e1aa2624 |
| zerocopy-derive | 0.8.56 | BSD-2-Clause OR Apache-2.0 OR MIT | f2ab42fc20575779bd240faa45f94a74256f755c0fa9e89f0ede20d91d0cdfc1 | 9d185ac6703c4b04, 83c1763356e822ad, 1a2f5c12ddc934d5 |
| zerocopy | 0.8.56 | BSD-2-Clause OR Apache-2.0 OR MIT | 556764e583adb45a9f8d413c2a147fa7e8d821e48e12b14fd560b607998b75eb | 9d185ac6703c4b04, 83c1763356e822ad, 1a2f5c12ddc934d5 |
| zerofrom-derive | 0.1.7 | Unicode-3.0 | 11532158c46691caf0f2593ea8358fed6bbf68a0315e80aae9bd41fbade684a1 | f367c1b8e1aa2624 |
| zerofrom | 0.1.8 | Unicode-3.0 | 0ec05a11813ea801ff6d75110ad09cd0824ddba17dfe17128ea0d5f68e6c5272 | f367c1b8e1aa2624 |
| zeroize | 1.9.0 | Apache-2.0 OR MIT | e13c156562582aa81c60cb29407084cdb54c4164760106ab78e6c5b0858cf64e | cfc7749b96f63bd3, 8c7516d4b27b1e49 |
| zerotrie | 0.2.5 | Unicode-3.0 | 4ea269c3bd32f0a32c321907a2ae912ba6f4649bb0fc764a15627e99a7095a3f | f367c1b8e1aa2624 |
| zerovec-derive | 0.11.6 | Unicode-3.0 | 34df6fc39dbd26ddc9c10e6a2984476e13acce22e64e4487636ef494369225da | f367c1b8e1aa2624 |
| zerovec | 0.11.8 | Unicode-3.0 | bb0464e17806c1d976d5cba29399c7f08e516e279e2ba493f63123b5fca67dd8 | f367c1b8e1aa2624 |
| zip | 4.6.1 | MIT | caa8cd6af31c3b31c6631b8f483848b91589021b28fffe50adada48d4f4d2ed1 | 04dc52136d82400b |
| zlib-rs | 0.6.7 | Zlib | 34b31d188d9d685a4f9c7b46d6e36631b07058d2cfe190267adce54dc230bf12 | e72111c52b7d96eb |
| zmij | 1.0.23 | MIT | 29666d0abbfad1e3dc4dcf6144730dd3a3ab225bbbdac83319345b1b44ccfc1b | 23f18e03dc49df91 |
| zopfli | 0.8.3 | Apache-2.0 | f05cd8797d63865425ff89b5c4a48804f35ba0ce8d125800027ad6017d2b5249 | 018b1cb87efdf7a0 |
