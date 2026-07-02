import { apiDelete, apiGet, apiRequest } from './index'

/**
 * Prompt-template store API (`/api/prompt-templates`).
 *
 * A "template" here is USER content — a saved simulation prompt plus its seed documents —
 * stored server-side under `{upload_folder}/templates/prompts/`. This is distinct from the
 * read-only `/api/templates` viewer (the engine's compiled LLM prompts).
 */

/**
 * List saved templates (newest first). Each item: { id, name, prompt, created_at, seeds: [filename] }.
 * @returns {Promise<Array>}
 */
export function listPromptTemplates() {
  return apiGet('/api/prompt-templates')
}

/**
 * Get one template's metadata (prompt + seed filenames).
 * @param {String} id
 * @returns {Promise<Object>}
 */
export function getPromptTemplate(id) {
  return apiGet(`/api/prompt-templates/${encodeURIComponent(id)}`)
}

/**
 * Save (create or overwrite) a template from the current prompt + seed files.
 * @param {String} name - display name
 * @param {String} prompt - simulation prompt text
 * @param {File[]} files - seed documents
 * @returns {Promise<Object>} { success, template }
 */
export function savePromptTemplate(name, prompt, files) {
  const formData = new FormData()
  formData.append('name', name)
  formData.append('prompt', prompt)
  for (const file of files) {
    formData.append('files', file, file.name)
  }
  return apiRequest({
    url: '/api/prompt-templates',
    method: 'post',
    data: formData,
    headers: { 'Content-Type': 'multipart/form-data' }
  })
}

/**
 * Delete a saved template.
 * @param {String} id
 * @returns {Promise<Object>} { success }
 */
export function deletePromptTemplate(id) {
  return apiDelete(`/api/prompt-templates/${encodeURIComponent(id)}`)
}

/**
 * Download one seed document and reconstruct it as a browser `File` (so it can be re-submitted
 * to the seed-upload pipeline exactly like a freshly-uploaded file).
 * @param {String} id - template id
 * @param {String} filename - seed filename
 * @returns {Promise<File>}
 */
export async function seedAsFile(id, filename) {
  const blob = await apiRequest({
    url: `/api/prompt-templates/${encodeURIComponent(id)}/seeds/${encodeURIComponent(filename)}`,
    method: 'get',
    responseType: 'blob'
  })
  return new File([blob], filename)
}
