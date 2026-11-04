# Proposal

## Why

A change `jvm-posix-compat-strategy` (arquivada) decidiu a estratégia geral de portabilidade do HotSpot (camada POSIX mínima no kernel), mas registrou explicitamente como **Non-Goal** a decisão sobre carregamento dinâmico de bibliotecas: *"Decidir a estratégia de linkagem (dinâmica vs. estática) do JDK/JVM final — registrado como risco em aberto, não como decisão."* Esse risco continua em `docs/architecture/overview.md` (seção 6) como "sem decisão fechada" até hoje, mesmo depois de quatro das cinco primitivas POSIX do inventário original já estarem entregues (`kernel-wait-queue`, `kernel-posix-virtual-memory`, `kernel-pthread-sync`, `kernel-wall-clock`, `kernel-jvm-posix-syscalls`, `kernel-safepoint-signal`, `kernel-fat32-filesystem`). Antes de abrir a próxima sub-change de implementação do item 8 do roadmap, essa decisão precisa ser fechada — exatamente como a mesma change antecipava que aconteceria.

## What Changes

- Decide formalmente a estratégia de carregamento dinâmico de bibliotecas: **rejeita** a rota de build totalmente estático do JDK/Leyden como inviável nesta etapa, e **adota** a implementação de um linker ELF dinâmico mínimo no próprio kernel como caminho a seguir — com base em pesquisa real sobre o estado atual (2025) do esforço "Hermetic Java"/build estático do OpenJDK/Leyden (ver design.md para as fontes e o raciocínio completo).
- Documenta um achado novo, não previsto em nenhuma change anterior: mesmo a rota estática (um único binário `javastatic`) ainda exigiria que o kernel soubesse **carregar um executável ELF do disco** — algo que não existe hoje (todo código de modo usuário atual, `kernel::user_demo`/`kernel::user_posix_demo`, é um blob de assembly copiado manualmente para uma página, sem nenhum carregador de verdade). Carregar um executável ELF estático e fazer o linker dinâmico de bibliotecas compartilhadas (`.so`, `dlopen`/`dlsym`) são dois problemas relacionados, mas distintos — o primeiro é pré-requisito do segundo, independentemente da estratégia de linkagem do JDK em si.
- Delimita a sequência das próximas sub-changes do item 8 do roadmap: primeiro um **carregador de executável ELF estático** (carrega e mapeia os segmentos `PT_LOAD` de um binário já linkado, sem nenhum carregamento dinâmico de `.so` ainda — escopo pequeno e imediato, já que nada disso existe hoje), depois o **linker ELF dinâmico mínimo** (`.so`, símbolos, relocações, `dlopen`/`dlsym`) decidido aqui.
- **Nenhum código do kernel muda nesta change** — é puramente uma decisão de arquitetura documentada (`proposal.md`/`design.md`), mesmo formato de `jvm-posix-compat-strategy`.

## Capabilities

### New Capabilities

Nenhuma — esta change não introduz nem modifica nenhum comportamento observável do sistema (nenhum código muda). Ela documenta uma decisão que condiciona o design das próximas sub-changes de implementação (carregador de ELF estático, depois linker dinâmico), mas essas capacidades ainda não existem e não são criadas aqui.

### Modified Capabilities

Nenhuma.

## Impact

- `docs/architecture/overview.md`: seção 6 ("Estratégia de Portabilidade do OpenJDK") passa do risco "sem decisão fechada" para a decisão tomada (linker ELF dinâmico mínimo, build estático rejeitado) e o achado sobre o carregador de ELF estático como pré-requisito separado; o item 8 do roadmap (seção 15) ganha a próxima sub-change explicitamente nomeada (carregador de ELF estático).
- Nenhum código de `kernel/` ou `xtask/` é alterado.
- Condiciona diretamente o escopo da próxima sub-change de implementação do item 8: um carregador de executável ELF estático (parsing de cabeçalho ELF64/*program headers*, mapeamento dos segmentos `PT_LOAD` via `kernel::vmm`/`kernel::paging`, leitura do binário via `kernel::fat32`), sem nenhum linker dinâmico ainda.
