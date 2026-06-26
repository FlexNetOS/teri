<template>
  <div class="templates-view">
    <!-- Top navigation -->
    <nav class="navbar">
      <div class="nav-brand" @click="router.push('/')">TERI</div>
      <div class="nav-center">{{ $t('templates.title') }}</div>
      <div class="nav-links">
        <LanguageSwitcher />
        <a class="back-link" @click="router.push('/')">
          {{ $t('templates.backHome') }} <span class="arrow">↩</span>
        </a>
      </div>
    </nav>

    <div class="content">
      <header class="page-header">
        <h1 class="page-title">{{ $t('templates.title') }}</h1>
        <p class="page-subtitle">{{ $t('templates.subtitle') }}</p>
      </header>

      <!-- Loading -->
      <div v-if="loading" class="state-msg">{{ $t('templates.loading') }}</div>

      <!-- Error -->
      <div v-else-if="error" class="state-msg state-error">{{ error }}</div>

      <!-- Empty -->
      <div v-else-if="templates.length === 0" class="state-msg">
        {{ $t('templates.empty') }}
      </div>

      <!-- Grouped by stage 1 → 5 -->
      <div v-else class="stage-groups">
        <section
          v-for="group in groupedTemplates"
          :key="group.stage"
          class="stage-group"
        >
          <h2 class="stage-heading">
            <span class="stage-num">{{ group.stage }}</span>
            {{ stageLabel(group.stage) }}
            <span class="stage-count">{{ group.items.length }}</span>
          </h2>

          <article
            v-for="tpl in group.items"
            :key="tpl.id"
            class="template-card"
            :class="{ open: isOpen(tpl.id) }"
          >
            <button class="card-header" @click="toggle(tpl.id)">
              <span class="card-toggle">{{ isOpen(tpl.id) ? '▾' : '▸' }}</span>
              <span class="card-name">{{ tpl.name }}</span>
              <span class="kind-badge" :class="`kind-${tpl.kind}`">
                {{ kindLabel(tpl.kind) }}
              </span>
              <span class="card-source">{{ tpl.source_path }}</span>
            </button>

            <div v-show="isOpen(tpl.id)" class="card-body">
              <div class="source-row">
                <span class="source-label">{{ $t('templates.sourcePath') }}:</span>
                <code class="source-code">{{ tpl.source_path }}</code>
              </div>
              <pre class="template-content"><code>{{ tpl.content }}</code></pre>
            </div>
          </article>
        </section>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import LanguageSwitcher from '../components/LanguageSwitcher.vue'
import { getTemplates } from '../api/templates'

const router = useRouter()
const { t } = useI18n()

const templates = ref([])
const loading = ref(true)
const error = ref('')
const openIds = ref(new Set())

const STAGES = [1, 2, 3, 4, 5]

// Group templates by stage, preserving stage order 1 → 5.
const groupedTemplates = computed(() => {
  return STAGES.map((stage) => ({
    stage,
    items: templates.value.filter((tpl) => tpl.stage === stage)
  })).filter((group) => group.items.length > 0)
})

const stageLabel = (stage) => t(`templates.stages.${stage}`)

const kindLabel = (kind) => {
  const key = `templates.kinds.${kind}`
  const label = t(key)
  // Fall back to the raw kind if no translation key exists.
  return label === key ? kind : label
}

const isOpen = (id) => openIds.value.has(id)

const toggle = (id) => {
  const next = new Set(openIds.value)
  if (next.has(id)) {
    next.delete(id)
  } else {
    next.add(id)
  }
  openIds.value = next
}

onMounted(async () => {
  try {
    const data = await getTemplates()
    templates.value = Array.isArray(data) ? data : []
    // Open the first template of each stage by default for discoverability.
    const firstOfStage = new Set()
    const open = new Set()
    for (const tpl of templates.value) {
      if (!firstOfStage.has(tpl.stage)) {
        firstOfStage.add(tpl.stage)
        open.add(tpl.id)
      }
    }
    openIds.value = open
  } catch (e) {
    console.error('Failed to load templates:', e)
    error.value = t('templates.loadError')
  } finally {
    loading.value = false
  }
})
</script>

<style scoped>
.templates-view {
  --black: #000000;
  --white: #ffffff;
  --orange: #ff4500;
  --gray-light: #f5f5f5;
  --gray-border: #e0e0e0;
  --gray-text: #666666;
  --font-mono: 'JetBrains Mono', monospace;

  min-height: 100vh;
  background: var(--white);
  color: var(--black);
  font-family: var(--font-mono);
}

/* Navbar (mirrors Home.vue) */
.navbar {
  height: 60px;
  background: var(--black);
  color: var(--white);
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0 40px;
}

.nav-brand {
  font-weight: 800;
  letter-spacing: 1px;
  font-size: 1.2rem;
  cursor: pointer;
}

.nav-center {
  font-size: 0.95rem;
  font-weight: 600;
  letter-spacing: 1px;
  color: var(--orange);
}

.nav-links {
  display: flex;
  align-items: center;
  gap: 16px;
}

.back-link {
  color: var(--white);
  text-decoration: none;
  font-size: 0.9rem;
  font-weight: 500;
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  transition: opacity 0.2s;
}

.back-link:hover {
  opacity: 0.8;
}

.arrow {
  font-family: sans-serif;
}

/* Content */
.content {
  max-width: 1100px;
  margin: 0 auto;
  padding: 48px 40px 80px;
}

.page-header {
  border-bottom: 2px solid var(--black);
  padding-bottom: 20px;
  margin-bottom: 32px;
}

.page-title {
  font-size: 1.8rem;
  font-weight: 800;
  letter-spacing: 0.5px;
}

.page-subtitle {
  margin-top: 8px;
  color: var(--gray-text);
  font-size: 0.9rem;
  line-height: 1.5;
}

.state-msg {
  padding: 40px;
  text-align: center;
  color: var(--gray-text);
  border: 1px dashed var(--gray-border);
}

.state-error {
  color: var(--orange);
  border-color: var(--orange);
}

/* Stage groups */
.stage-group {
  margin-bottom: 40px;
}

.stage-heading {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 1.1rem;
  font-weight: 700;
  margin-bottom: 16px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.stage-num {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  background: var(--orange);
  color: var(--white);
  font-size: 0.95rem;
  font-weight: 800;
}

.stage-count {
  margin-left: auto;
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--gray-text);
}

/* Template card */
.template-card {
  border: 1px solid var(--gray-border);
  margin-bottom: 12px;
  background: var(--white);
}

.template-card.open {
  border-color: var(--black);
}

.card-header {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 16px;
  background: var(--gray-light);
  border: none;
  cursor: pointer;
  font-family: var(--font-mono);
  text-align: left;
}

.card-toggle {
  color: var(--orange);
  font-size: 0.85rem;
}

.card-name {
  font-weight: 700;
  font-size: 0.9rem;
}

.kind-badge {
  font-size: 0.68rem;
  font-weight: 600;
  text-transform: uppercase;
  padding: 2px 8px;
  border: 1px solid var(--black);
  letter-spacing: 0.5px;
}

.kind-jinja {
  background: var(--black);
  color: var(--white);
}

.kind-system_prompt {
  background: var(--orange);
  color: var(--white);
  border-color: var(--orange);
}

.kind-user_prompt {
  background: var(--white);
  color: var(--black);
}

.card-source {
  margin-left: auto;
  font-size: 0.72rem;
  color: var(--gray-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 40%;
}

.card-body {
  padding: 16px;
  border-top: 1px solid var(--gray-border);
}

.source-row {
  display: flex;
  gap: 8px;
  align-items: baseline;
  margin-bottom: 12px;
  font-size: 0.78rem;
  flex-wrap: wrap;
}

.source-label {
  color: var(--gray-text);
  font-weight: 600;
}

.source-code {
  color: var(--black);
  word-break: break-all;
}

.template-content {
  background: var(--black);
  color: #e8e8e8;
  padding: 16px;
  overflow-x: auto;
  font-size: 0.82rem;
  line-height: 1.5;
  white-space: pre;
  font-family: var(--font-mono);
}

.template-content code {
  font-family: inherit;
}
</style>
