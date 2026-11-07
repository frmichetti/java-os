# Proposal

## Why

O projeto já tem 24 changes arquivadas e uma arquitetura de 15 áreas documentada em `docs/architecture/overview.md`, mas essa informação está espalhada (uma seção densa de ~700 linhas, mais o histórico individual de cada change em `openspec/changes/archive/`). Não existe hoje um documento único, enxuto e no topo do repositório que responda diretamente "o que já funciona?" e "o que falta para rodar Java e ter uma interface gráfica?" — as duas perguntas que o usuário mais precisa responder a qualquer momento para situar o projeto. Um `ROADMAP.md` na raiz, atualizado a cada marco, preenche esse gap sem substituir a arquitetura técnica detalhada (que continua em `docs/architecture/overview.md`) nem o `SPECIFICATION.md` (regras de negócio/casos de uso).

## What Changes

- Novo arquivo `ROADMAP.md` na raiz do repositório, com:
  - O que já está implementado e verificado em boot real, organizado por área (kernel, memória, processos/threads, syscalls, HAL/drivers, FPU/ponto flutuante, libc C, carregamento de executáveis/linker dinâmico).
  - O que está previsto, organizado por horizonte (próximo passo imediato, médio prazo, longo prazo), reaproveitando e resumindo o roadmap já existente em `docs/architecture/overview.md`, seção 15.
  - Uma seção dedicada e destacada: "o que falta para a JVM rodar" (porte do HotSpot/`os_javaos`, libm/ponto flutuante completo na libc, isolamento de falha por processo, etc.) e "o que falta para uma interface gráfica" (primitivas de desenho via syscall, driver de mouse, compositor, toolkit — hoje só framebuffer bruto + console de texto de depuração).
- Não introduz nem modifica nenhum comportamento do sistema — mudança puramente documental (`skip_specs: true`).

## Capabilities

### New Capabilities

(nenhuma — mudança documental, sem comportamento de sistema)

### Modified Capabilities

(nenhuma)

## Impact

- **Código afetado**: nenhum.
- **Arquivos afetados**: novo `ROADMAP.md` na raiz. Nenhuma alteração em `docs/architecture/overview.md`/`SPECIFICATION.md` — o `ROADMAP.md` resume e referencia esses documentos, não os substitui nem duplica seu nível de detalhe.
- **Achado relevante para o conteúdo**: `SPECIFICATION.md` (gerado por uma change concluída em paralelo a esta sessão) já contém uma seção "20. ROADMAP DE MODERNIZAÇÃO", mas ela ficou desatualizada antes mesmo de ser arquivada — lista `FXSAVE`/`FXRSTOR` por thread como pendência de "curto prazo", quando a change `kernel-fpu-context-switch` já entregou isso (arquivada na mesma sessão). O `ROADMAP.md` desta change reflete o estado real e atual verificado em boot real, não aquele snapshot.
