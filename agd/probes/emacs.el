(require 'cl-lib)
(require 'seq)
(require 'subr-x)
(require 'json)
(require 'org)
(require 'org-agenda)
(require 'org-capture nil t)

(defvar agd-import-verify nil)

(defun agd--string (value)
  (cond ((null value) "nil")
        ((stringp value) value)
        ((symbolp value) (symbol-name value))
        ((characterp value) (char-to-string value))
        (t (format "%S" value))))

(defun agd--strings (values)
  (vconcat (mapcar #'agd--string (or values '()))))

(defun agd--todo-sequences ()
  (vconcat
   (mapcar (lambda (sequence)
             (vconcat (mapcar #'agd--string (cdr sequence))))
           org-todo-keywords)))

(defun agd--capture-template (entry)
  (let* ((key (nth 0 entry))
         (description (nth 1 entry))
         (kind (nth 2 entry))
         (target (nth 3 entry))
         (body (nth 4 entry))
         (enabled (eq kind 'entry))
         (diagnostics '())
         (value (make-hash-table :test 'equal)))
    (puthash "desc" description value)
    (puthash "datetree" :json-false value)
    (puthash "enabled" (if enabled t :json-false) value)
    (puthash "diagnostics" [] value)
    (cond
     ((stringp body)
      (puthash "template" body value)
      (when (string-match-p "%[(]" body)
        (setq enabled nil diagnostics '("%(sexp) template forms are disabled"))))
     ((and (listp body) (eq (car body) 'file) (stringp (cadr body)))
      (puthash "template" "" value)
      (puthash "template_file" (expand-file-name (cadr body)) value))
     (t
      (puthash "template" "" value)
      (setq enabled nil diagnostics '("dynamic template body is unsupported"))))
    (cond
     ((and (listp target) (eq (car target) 'file) (stringp (cadr target)))
      (puthash "file" (expand-file-name (cadr target)) value))
     ((and (listp target) (eq (car target) 'file+headline)
           (stringp (cadr target)) (stringp (nth 2 target)))
      (puthash "file" (expand-file-name (cadr target)) value)
      (puthash "heading" (nth 2 target) value))
     ((and (listp target) (eq (car target) 'file+olp) (stringp (cadr target)))
      (puthash "file" (expand-file-name (cadr target)) value)
      (puthash "outline_path" (vconcat (cddr target)) value))
     ((and (listp target) (eq (car target) 'file+datetree) (stringp (cadr target)))
      (puthash "file" (expand-file-name (cadr target)) value)
      (puthash "datetree" t value))
     ((and (listp target) (eq (car target) 'id) (stringp (cadr target)))
      (puthash "id" (cadr target) value))
     (t (setq enabled nil diagnostics
              (cons "dynamic capture target is unsupported" diagnostics))))
    (puthash "enabled" (if enabled t :json-false) value)
    (puthash "diagnostics" (vconcat diagnostics) value)
    (cons key value)))

(defun agd--captures ()
  (let ((result (make-hash-table :test 'equal)))
    (dolist (entry (or (and (boundp 'org-capture-templates) org-capture-templates) '()))
      (when (and (listp entry) (>= (length entry) 5))
        (let ((normalized (agd--capture-template entry)))
          (puthash (car normalized) (cdr normalized) result))))
    result))

;; Lambdas, #'symbol forms and compiled functions; a bare symbol such as
;; `org-agenda-files' in an option value is data, not code.
(defun agd--contains-function-p (value)
  (cond
   ((and (consp value) (memq (car value) '(lambda function closure))) t)
   ((consp value)
    (or (agd--contains-function-p (car value))
        (agd--contains-function-p (cdr value))))
   ((and value (not (symbolp value)) (functionp value)) t)
   (t nil)))

;; org-agenda-custom-commands -> views. Only literal option values are read;
;; nothing from the user's configuration is evaluated.
(defconst agd--view-kinds
  '((agenda . "agenda") (todo . "todo") (alltodo . "alltodo")
    (tags . "tags") (tags-todo . "tags-todo")))

(defun agd--literal (form)
  (cond ((and (consp form) (eq (car form) 'quote)) (cadr form))
        ((or (stringp form) (numberp form) (symbolp form)) form)
        (t :agd-dynamic)))

(defun agd--view-block (kind match options notes)
  "Return (BLOCK . NOTES) for one block, or nil when KIND is unsupported."
  (let ((name (cdr (assq kind agd--view-kinds)))
        (block (make-hash-table :test 'equal)))
    (when name
      (puthash "kind" name block)
      (when (and (stringp match) (not (string-empty-p match)))
        (puthash "match" match block))
      (dolist (option options)
        (let ((symbol (car-safe option))
              (value (agd--literal (car-safe (cdr-safe option)))))
          (cond
           ((and (eq symbol 'org-agenda-span)
                 (memq value '(day week month)))
            (puthash "span" (symbol-name value) block))
           ((and (eq symbol 'org-agenda-span) (integerp value))
            (puthash "span" (number-to-string value) block))
           ((and (eq symbol 'org-agenda-overriding-header) (stringp value))
            (puthash "header" value block))
           (t (push (format "option %s is not imported" symbol) notes)))))
      (cons block notes))))

;; The description is optional: (KEY TYPE MATCH ...) is also valid.
(defun agd--portable-command (entry)
  (when (not (stringp (nth 1 entry)))
    (setq entry (append (list (car entry) nil) (cdr entry))))
  (let* ((key (agd--string (nth 0 entry)))
         (description (or (nth 1 entry) key))
         (third (nth 2 entry))
         (blocks '())
         (notes '())
         (supported (not (agd--contains-function-p entry))))
    (if (and (consp third) (consp (car third)))
        ;; Block agenda: (KEY DESC ((TYPE MATCH OPTIONS) ...) OPTIONS FILES)
        (dolist (spec third)
          (let ((result (agd--view-block (nth 0 spec) (nth 1 spec)
                                         (append (nth 2 spec) (nth 3 entry))
                                         notes)))
            (if result
                (progn (push (car result) blocks) (setq notes (cdr result)))
              (setq supported nil))))
      ;; Single command: (KEY DESC TYPE MATCH OPTIONS FILES)
      (let ((result (agd--view-block third (nth 3 entry) (nth 4 entry) notes)))
        (if result
            (progn (push (car result) blocks) (setq notes (cdr result)))
          (setq supported nil))))
    `((key . ,key)
      (description . ,description)
      (enabled . ,(if (and supported blocks) t :json-false))
      (blocks . ,(vconcat (nreverse blocks)))
      (diagnostics . ,(vconcat (if supported (delete-dups (nreverse notes))
                                 '("custom command form is unsupported")))))))

(defun agd--commands ()
  (vconcat
   (mapcar #'agd--portable-command
           (seq-filter (lambda (entry) (and (consp entry) (> (length entry) 2)))
                       (or (and (boundp 'org-agenda-custom-commands)
                                org-agenda-custom-commands)
                           '())))))

(defconst agd--super-agenda-keys
  '(:name :tag :priority :todo :deadline :scheduled :time-grid :and :not :auto-category))

(defun agd--super-group-supported-p (group)
  (and (boundp 'org-super-agenda-version)
       (string-match-p "\\`1\\." (agd--string org-super-agenda-version))
       (listp group)
       (not (agd--contains-function-p group))
       (cl-loop for (key _value) on group by #'cddr
                always (memq key agd--super-agenda-keys))))

(defun agd--super-groups ()
  (if (boundp 'org-super-agenda-groups)
      (vconcat
       (mapcar
        (lambda (group)
          (let ((supported (agd--super-group-supported-p group)))
            `((key . ,(agd--string (or (plist-get group :name) "group")))
              (description . ,(agd--string (or (plist-get group :name) "Group")))
              (kind . "org-super-agenda")
              (match_expression . ,(format "%S" group))
              (enabled . ,(if supported t :json-false))
              (diagnostics . ,(if supported []
                                [\"unsupported org-super-agenda version or group form\"])))))
        org-super-agenda-groups))
    []))

;; The agenda part of org-agenda-sorting-strategy. Before Org 9.7 the
;; priority-* keys sorted by what 9.7 renamed urgency.
(defun agd--sorting-strategy ()
  (let* ((value org-agenda-sorting-strategy)
         (agenda (if (assq 'agenda value) (cdr (assq 'agenda value)) value)))
    (vconcat
     (mapcar (lambda (key)
               (let ((name (agd--string key)))
                 (if (version< (org-version) "9.7")
                     (replace-regexp-in-string "\\`priority-" "urgency-" name)
                   name)))
             agenda))))

(defun agd--verification-rows ()
  (let ((result (make-hash-table :test 'equal)))
    (when (or agd-import-verify
              (equal (getenv "AGD_IMPORT_VERIFY") "1"))
      (puthash "a" (with-output-to-string (org-batch-agenda-csv "a")) result)
      (dolist (entry (or (and (boundp 'org-agenda-custom-commands)
                              org-agenda-custom-commands)
                         '()))
        (when (and (listp entry)
                   (stringp (car entry))
                   (> (length entry) 2)
                   (not (agd--contains-function-p entry)))
          (condition-case error
              (puthash (car entry)
                       (with-output-to-string
                         (org-batch-agenda-csv (car entry)))
                       result)
            (error
             (puthash (car entry)
                      (format "AGD_VERIFY_ERROR:%s"
                              (error-message-string error))
                      result))))))
    result))

(defun agd-import-probe ()
  (let ((capture (agd--captures)))
    (json-encode
     `((editor . "emacs")
       (editor_version . ,emacs-version)
       (org_version . ,(org-version))
       (source . ,(or (getenv "AGD_IMPORT_SOURCE") "daemon"))
       (agenda_files . ,(vconcat (org-agenda-files t)))
       (todo_sequences . ,(agd--todo-sequences))
       (tag_alist . ,(agd--strings org-tag-alist))
       (persistent_tags . ,(agd--strings org-tag-persistent-alist))
       (tag_inheritance_exclusions . ,(agd--strings org-tags-exclude-from-inheritance))
       (highest_priority . ,(char-to-string org-highest-priority))
       (lowest_priority . ,(char-to-string org-lowest-priority))
       (default_priority . ,(char-to-string org-default-priority))
       (deadline_warning_days . ,org-deadline-warning-days)
       (agenda_span . ,(agd--string org-agenda-span))
       (agenda_start_day . ,org-agenda-start-day)
       (week_start . ,(if (= org-agenda-start-on-weekday 0) "sunday" "monday"))
       (skip_scheduled_if_done . ,(if org-agenda-skip-scheduled-if-done t :json-false))
       (skip_deadline_if_done . ,(if org-agenda-skip-deadline-if-done t :json-false))
       (skip_timestamp_if_done . ,(if org-agenda-skip-timestamp-if-done t :json-false))
       (sorting_strategy . ,(agd--sorting-strategy))
       (log_done . ,(agd--string org-log-done))
       (log_repeat . ,(agd--string org-log-repeat))
       (log_into_drawer . ,(if org-log-into-drawer t :json-false))
       (capture . ,capture)
       (groups . ,(agd--super-groups))
       (commands . ,(agd--commands))
       (verification . ,(agd--verification-rows))
       (diagnostics . [])))))

(when (equal (getenv "AGD_PROBE_STDOUT") "1")
  (princ "AGD_JSON:")
  (princ (agd-import-probe))
  (terpri))
