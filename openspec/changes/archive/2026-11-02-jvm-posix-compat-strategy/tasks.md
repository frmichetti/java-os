# Tasks

## 1. Atualizar a documentação de arquitetura com a decisão

- [x] 1.1 Reescrever a seção 6 de `docs/architecture/overview.md` ("Estratégia de Portabilidade do OpenJDK"): substituir as três opções em aberto pela decisão tomada (opção 1 — camada de compatibilidade POSIX mínima), incluir o inventário de primitivas exigidas (memória virtual, threads/sincronização, sinal de safepoint, relógio, E/S de arquivo) e o que falta no kernel para cada uma, e registrar o achado sobre carregamento dinâmico de bibliotecas como risco em aberto para a change do item 8 — ver design.md ("Decisions"/"Riscos / Trade-offs"). Verificar lendo a seção reescrita.
- [x] 1.2 Atualizar o item 7 do roadmap (seção 15 de `docs/architecture/overview.md`), marcando-o como **concluído** com um resumo de uma frase da decisão tomada, no mesmo estilo dos itens 1-6 já concluídos. Verificar lendo o item atualizado.

## Workflow follow-up

- Arquivar esta change após a aprovação do usuário para a decisão documentada.
