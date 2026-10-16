(require 'cl-lib)
(require 'org)

;; Applies one edit with the case's org-log-* settings and a frozen clock,
;; then runs post-command-hook so Org stores the log note as it would after
;; an interactive command.
(defun agd--setting (name)
  (let ((value (getenv name)))
    (cond ((or (null value) (string= value "nil")) nil)
          ((string= value "t") t)
          (t (intern value)))))

(let* ((operation (or (getenv "AGD_EDIT_OPERATION")
                      (error "AGD_EDIT_OPERATION is required")))
       (value (getenv "AGD_EDIT_VALUE"))
       (now (apply #'encode-time
                   (parse-time-string (concat (getenv "AGD_NOW") ":00")))))
  (setq org-log-done (agd--setting "AGD_LOG_DONE")
        org-log-repeat (agd--setting "AGD_LOG_REPEAT")
        org-log-reschedule (agd--setting "AGD_LOG_RESCHEDULE")
        org-log-redeadline (agd--setting "AGD_LOG_REDEADLINE")
        org-log-into-drawer (agd--setting "AGD_LOG_INTO_DRAWER"))
  ;; org-today reads the clock through the time-since primitive, which a
  ;; current-time override does not reach, so freeze both.
  (cl-letf (((symbol-function 'current-time) (lambda () now))
            ((symbol-function 'org-today) (lambda () (time-to-days now))))
    (find-file "/root/input.org")
    (goto-char (point-min))
    (unless (re-search-forward "^[ \t]*:ID:[ \t]+task-id[ \t]*$" nil t)
      (error "task-id not found"))
    (org-back-to-heading t)
    (pcase operation
      ("state" (org-todo value))
      ("schedule" (org-schedule nil value))
      ("deadline" (org-deadline nil value))
      ("priority" (org-priority (string-to-char value)))
      ;; VALUE is the box's place in the entry's checklist, 0 first.
      ("checkbox"
       (let ((end (save-excursion (outline-next-heading) (point))))
         (dotimes (_ (1+ (string-to-number value)))
           (re-search-forward
            "^[ \t]*\\(?:[-+*]\\|[0-9]+[.)]\\)[ \t]+\\[[ X-]\\]" end))
         (org-toggle-checkbox)))
      (_ (error "unsupported edit operation: %s" operation)))
    (run-hooks 'post-command-hook)
    (save-buffer)))
