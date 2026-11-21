# Proposal

## Why

`share/classfile/classLoader.hpp` inclui `jimage.hpp` (a API de leitura de
recursos do formato de imagem de módulos do JDK — `jrt-fs`/`lib/modules`),
nunca vendorizado de `src/java.base` (só `share/native/include`/
`unix/native/include` foram, pontualmente, por `os-javaos-skeleton`) —
achado registrado como bloqueio conhecido pela change
`jvmti-generated-headers` (arquivada). Isso impedia `classLoader.cpp` e,
por extensão, `universe.cpp`/`threads.cpp` de sequer serem syntax-checados
(`-fsyntax-only`).

Uma investigação do arquivo real (buscado do repositório oficial
`openjdk/jdk` na tag `jdk-27-ga`, numa cópia de teste fora do repo, ainda
não vendorizada) revelou que `jimage.hpp` é um header pequeno e
autocontido (~170 linhas): só depende de `jni.h` (já vendorizado), declara
`JImageFile` como classe opaca e 4 `typedef`s de ponteiro de função — o
HotSpot nunca linka contra uma implementação real de `libjimage`, carrega
essas 4 funções em runtime via `os::dll_load`/`dll_lookup`
(`ClassLoader::load_jimage_library`). Vendorizar só o header basta —
confirmado que, com ele presente, `classLoader.cpp` **e** `universe.cpp`
compilam limpo via `g++ -fsyntax-only`.

## What Changes

- Vendorizar `jimage.hpp` real (do repositório oficial `openjdk/jdk`, tag
  `jdk-27-ga`) em
  `vendor/jdk-27-ga/src/java.base/share/native/libjimage/jimage.hpp` —
  mesma convenção pontual já usada para `jni_md.h`/`classfile_constants.h`
  (achado de `os-javaos-skeleton`).
- Documentar a nova entrada de `-I` necessária para o comando de
  verificação `g++ -fsyntax-only`
  (`-I ../java.base/share/native/libjimage`, ao lado das duas entradas de
  `java.base` já existentes).
- Verificar por compilação real que `share/classfile/classLoader.cpp` e
  `share/memory/universe.cpp` compilam limpo com a nova entrada de `-I` —
  confirmando o que antes parecia o início do roadmap item 4
  (`classfile`/`oops`, ~83+125 arquivos) na verdade não precisava de mais
  nada além deste único arquivo vendorizado.
- Atualizar `ROADMAP.md`: registrar o achado e sua resolução.

**Fora do escopo desta change**: `share/runtime/threads.cpp` continua
bloqueado pelo achado de JFR já documentado (`jfrfiles/jfrEventClasses.hpp`,
`jvmti-generated-headers`) — não relacionado a `jimage`/`classfile`, não
resolvido aqui. Nenhuma implementação real de `libjimage` (o HotSpot nunca
precisa dela em tempo de compilação, só em runtime via `dlopen`-style).
Qualquer outro arquivo do roadmap item 4 (`classfile`/`oops`) além dos dois
já verificados.

## Capabilities

### New Capabilities
- `hotspot-vendored-java-base-headers`: garante que headers de
  `src/java.base` exigidos transitivamente por código do HotSpot
  vendorizado são vendorizados pontualmente a partir do repositório real
  do OpenJDK, nunca reescritos ou simplificados por este projeto — mesmo
  princípio já estabelecido por `os-javaos-skeleton` para
  `jni_md.h`/`classfile_constants.h`, agora formalizado como capability
  própria.

## Impact

- `vendor/jdk-27-ga/src/java.base/share/native/libjimage/jimage.hpp`: novo
  arquivo vendorizado (não versionado no Git, mesma convenção do resto de
  `vendor/jdk-27-ga`).
- `ROADMAP.md`: registra o achado e a resolução.
- Nenhuma mudança em `kernel/`, `userland_c/`, ou em qualquer arquivo
  `.cpp`/`.hpp` do HotSpot em si — escopo inteiramente dentro da árvore
  vendorizada (um novo arquivo) e da documentação/comando de verificação.
