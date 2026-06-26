import service from './index'

/**
 * 获取驱动各流水线阶段的 LLM 提示词模板
 * Fetch the LLM prompt templates that drive each pipeline stage.
 *
 * Returns an array of descriptors:
 *   { id, stage, step_label, kind, name, source_path, content }
 * The response is a raw JSON array (no `success` envelope), so the shared
 * response interceptor passes it through unchanged.
 *
 * @returns {Promise<Array>} prompt template descriptors, ordered by stage
 */
export const getTemplates = () => {
  return service.get('/api/templates')
}
