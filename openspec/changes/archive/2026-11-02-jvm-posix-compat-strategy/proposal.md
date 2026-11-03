# Proposal

## Why

O item 6 do roadmap (HAL e drivers essenciais) está concluído — o kernel agora tem console, teclado, armazenamento e rede. O próximo passo de maior risco técnico do projeto (seção 6 de `docs/architecture/overview.md`) é decidir **como** uma JVM vai rodar sobre um kernel próprio e não-POSIX, antes de escrever qualquer código de porte de JVM (item 8 do roadmap). O HotSpot (e qualquer JVM derivada dele) assume por padrão um ambiente hospedeiro rico — `mmap`/`mprotect`, pthreads, sinais Unix, carregamento dinâmico de bibliotecas — que o Java OS não tem e não pretende replicar por completo. Sem essa decisão tomada e documentada agora, o item 8 não tem uma base de design para começar.

## What Changes

- Decide formalmente a estratégia de portabilidade: **opção 1** das três listadas na seção 6 do documento de arquitetura — uma camada de compatibilidade POSIX mínima implementada no kernel, para que o HotSpot (ou uma JVM derivada) rode sobre ela com o mínimo de modificação possível no próprio código-fonte do HotSpot.
- Documenta, com base em pesquisa real sobre a estrutura interna do HotSpot (camada `os`/`os_posix`/`os_linux`), quais primitivas POSIX são realmente exigidas no mínimo: reserva/commit de memória virtual (`mmap`/`mprotect`), threads nativas com `pthread_mutex`/`pthread_cond` (usadas por `PlatformEvent`/`Parker`, a base do `park`/`unpark` de toda concorrência Java), um mecanismo de sinais Unix (usado para o polling de safepoint via `SIGSEGV` em página protegida, e para tratamento de erro fatal), relógio monotônico/relógio de parede, e E/S de arquivo básica para carregar classes.
- Documenta um achado de pesquisa não previsto no roadmap original: o HotSpot "de fábrica" também depende de **carregamento dinâmico de bibliotecas compartilhadas** (`libjvm.so` carregando `libjava.so` via o linker dinâmico do sistema, e JNI carregando bibliotecas nativas de aplicações via `dlopen`) — um requisito de peso comparável aos já conhecidos (memória/threads/sinais), e que não tem solução óbvia num kernel sem um linker dinâmico de ELF. Registra como **risco a decidir numa change futura** (candidato principal: build totalmente estático do JDK, uma linha de trabalho já experimental no próprio projeto OpenJDK/Leyden), sem fechar essa decisão agora.
- Mapeia o escopo mínimo viável da camada de compatibilidade POSIX contra o que o kernel já tem (paginação própria, `kernel::scheduler`/`kernel::process`, `kernel::sync::Spinlock`) e o que falta, delimitando explicitamente o que fica para esta change (decisão + inventário) versus o que fica para o item 8 (porte inicial da JVM, que implementa a camada de verdade).
- **Nenhum código do kernel muda nesta change** — é puramente uma decisão de arquitetura documentada (proposal/design), análoga ao que o próprio roadmap já antecipa como uma "change dedicada" separada do porte em si.

## Capabilities

### New Capabilities

Nenhuma — esta change não introduz nem modifica nenhum comportamento observável do sistema (nenhum código muda). Ela documenta uma decisão de arquitetura que condiciona o design de uma capacidade futura (o porte da JVM, item 8 do roadmap), mas essa capacidade ainda não existe e não é criada aqui.

### Modified Capabilities

Nenhuma.

## Impact

- `docs/architecture/overview.md`: seção 6 ("Estratégia de Portabilidade do OpenJDK") passa de três opções em aberto para a decisão tomada (opção 1) e o inventário de primitivas exigidas; o item 7 do roadmap (seção 15) é marcado como concluído.
- Nenhum código de `kernel/` ou `xtask/` é alterado.
- Condiciona diretamente o design da próxima change (item 8 do roadmap, "Porte inicial da JVM"): define que primitivas novas o kernel precisará expor (ex.: uma futura API de reserva/commit de memória virtual separada da paginação interna atual, um mecanismo de instalação de handler de página protegida equivalente a `SIGSEGV`, primitivas de thread nativa expostas para além do escalonador interno) sem implementá-las ainda.
