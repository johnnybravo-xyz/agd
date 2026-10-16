;;; agd's Emacs config reader -*- lexical-binding: t; -*-
;;; Runs in `emacs -Q --batch -l` with
;;; AGD_IMPORT_FILES (newline-separated) naming the config files to read.
;;;
;;; The files are read with Emacs's reader and never loaded: only assignments
;;; to the org settings agd uses are collected, from the top level and from
;;; after!, with-eval-after-load, use-package (:custom, :init, :config),
;;; progn/when/unless and Spacemacs's user-config. Their values, plus the
;;; top-level variables those values use, are then evaluated in this clean
;;; Emacs: top-level forms first, then deferred bodies, each in file order.
;;; Prints AGD_JSON:<payload> on stdout.

(require 'cl-lib)
(require 'seq)
(require 'subr-x)
(require 'json)
(require 'org)
(require 'org-agenda)
(require 'org-capture nil t)

(defconst agd--imported
  '(org-directory org-default-notes-file org-agenda-files org-todo-keywords
    org-tag-alist org-tag-persistent-alist org-tags-column
    org-tags-exclude-from-inheritance
    org-highest-priority org-lowest-priority org-default-priority
    org-priority-highest org-priority-lowest org-priority-default
    org-deadline-warning-days org-agenda-span org-agenda-start-day
    org-agenda-start-on-weekday org-agenda-skip-scheduled-if-done
    org-agenda-skip-deadline-if-done org-agenda-skip-timestamp-if-done
    org-agenda-sorting-strategy org-log-done org-log-repeat
    org-log-reschedule org-log-redeadline org-log-into-drawer
    org-capture-templates org-agenda-custom-commands)
  "The org settings agd imports.")

(defconst agd--unused-prefixes
  '("org-agenda-" "org-super-agenda-" "org-ql-" "org-capture-")
  "Settings with these prefixes that agd does not import are reported as unused.")

(defvar agd--assignments nil
  "Collected (KIND SYMBOL VALUE PHASE FILE LINE), newest first. KIND is set,
default, add, append or push; PHASE 0 is top level, 1 deferred.")
(defvar agd--diagnostics nil)
(defvar agd--origins (make-hash-table) "Imported SYMBOL -> \"file:line\".")
(defvar agd--file nil)
(defvar agd--form-start nil)
(defvar agd--form-end nil)
(defvar agd-import-verify nil)

(defun agd--diagnostic (status key message)
  (push `((status . ,status) (key . ,key) (message . ,message)) agd--diagnostics))

(defun agd--line (symbol)
  "Line of SYMBOL's first mention in code (not a comment or string) in the
form being walked."
  (save-excursion
    (goto-char agd--form-start)
    (let ((pattern (concat "\\_<" (regexp-quote (symbol-name symbol)) "\\_>"))
          found)
      (while (and (not found) (re-search-forward pattern agd--form-end t))
        (unless (nth 8 (syntax-ppss))
          (setq found (line-number-at-pos))))
      (or found (line-number-at-pos agd--form-start)))))

(defun agd--note (kind symbol value phase)
  (when (and symbol (symbolp symbol))
    (push (list kind symbol value phase agd--file (agd--line symbol)) agd--assignments)))

(defun agd--quoted (form)
  "The symbol or list in (quote X) or (function X), else nil."
  (and (consp form) (memq (car form) '(quote function)) (cadr form)))

(defun agd--walk (form phase)
  (when (consp form)
    (pcase (car form)
      ((or 'setq 'setq-default 'setq! 'setopt 'csetq)
       (let ((args (cdr form)))
         (while (and (consp args) (symbolp (car args)))
           (agd--note 'set (car args) (cadr args) phase)
           (setq args (cddr args)))))
      ((or 'customize-set-variable 'set-default 'custom-set-default)
       (agd--note 'set (agd--quoted (nth 1 form)) (nth 2 form) phase))
      ('custom-set-variables
       (dolist (entry (cdr form))
         (let ((spec (agd--quoted entry)))
           (when (consp spec)
             (agd--note 'set (car spec) (cadr spec) phase)))))
      ('add-to-list
       (agd--note (if (nth 3 form) 'append 'add) (agd--quoted (nth 1 form)) (nth 2 form) phase))
      ('push
       (when (symbolp (nth 2 form))
         (agd--note 'push (nth 2 form) (nth 1 form) phase)))
      ('defconst (agd--note 'set (nth 1 form) (nth 2 form) phase))
      ((or 'defvar 'defcustom)
       (when (> (length form) 2)
         (agd--note 'default (nth 1 form) (nth 2 form) phase)))
      ('progn (dolist (sub (cdr form)) (agd--walk sub phase)))
      ((or 'when 'unless) (dolist (sub (cddr form)) (agd--walk sub phase)))
      ((or 'after! 'with-eval-after-load)
       (dolist (sub (cddr form)) (agd--walk sub 1)))
      ('eval-after-load
       (agd--walk (or (agd--quoted (nth 2 form)) (nth 2 form)) 1))
      ((or 'use-package 'use-package!)
       (agd--walk-use-package (cddr form) phase))
      ('defun
       (when (memq (nth 1 form) '(dotspacemacs/user-config dotspacemacs/user-init))
         (dolist (sub (nthcdr 3 form)) (agd--walk sub phase)))))))

(defun agd--walk-use-package (args phase)
  (let (keyword)
    (dolist (arg args)
      (if (keywordp arg)
          (setq keyword arg)
        (pcase keyword
          (:custom
           (if (and (consp arg) (symbolp (car arg)))
               (agd--note 'set (car arg) (cadr arg) phase)
             (dolist (entry arg)
               (when (consp entry) (agd--note 'set (car entry) (cadr entry) phase)))))
          (:init (agd--walk arg phase))
          (:config (agd--walk arg 1)))))))

(defun agd--src-block-regions ()
  "Regions of the emacs-lisp source blocks in this Org buffer, except :tangle no."
  (let ((case-fold-search t) regions)
    (goto-char (point-min))
    (while (re-search-forward "^[ \t]*#\\+begin_src[ \t]+\\(?:emacs-lisp\\|elisp\\)\\(.*\\)$" nil t)
      (let ((arguments (match-string 1))
            (start (1+ (line-end-position))))
        (when (re-search-forward "^[ \t]*#\\+end_src" nil t)
          (unless (string-match-p ":tangle[ \t]+no" arguments)
            (push (cons start (line-beginning-position)) regions)))))
    (nreverse regions)))

(defun agd--read-file (file)
  "Walk every form in FILE (its emacs-lisp blocks, for an Org file)."
  (with-temp-buffer
    (insert-file-contents file)
    (emacs-lisp-mode)
    (let ((agd--file (file-name-nondirectory file))
          (regions (if (string-suffix-p ".org" file)
                       (agd--src-block-regions)
                     (list (cons (point-min) (point-max))))))
      (dolist (region regions)
        (save-restriction
          (narrow-to-region (car region) (cdr region))
          (goto-char (point-min))
          (condition-case error
              (while (progn (forward-comment (buffer-size)) (not (eobp)))
                (let* ((agd--form-start (point))
                       (form (read (current-buffer)))
                       (agd--form-end (point)))
                  (agd--walk form 0)))
            (error
             (agd--diagnostic "unsupported" agd--file
                              (format "stopped reading at line %d: %s"
                                      (line-number-at-pos)
                                      (error-message-string error))))))))))

(defun agd--symbols (form)
  "Symbols mentioned anywhere in FORM."
  (cond ((and form (symbolp form)) (list form))
        ((consp form) (append (agd--symbols (car form)) (agd--symbols (cdr form))))
        (t nil)))

(defun agd--selected ()
  "Assignments to evaluate, in order: imported settings plus the top-level
assignments of variables their values use, to a fixpoint; top level first,
then deferred, each in file order."
  (let* ((all (reverse agd--assignments))
         (selected (seq-filter (lambda (entry) (memq (nth 1 entry) agd--imported)) all))
         (grew t))
    (while grew
      (setq grew nil)
      (let ((used (delete-dups (apply #'append (mapcar (lambda (entry) (agd--symbols (nth 2 entry))) selected)))))
        (dolist (entry all)
          (when (and (= (nth 3 entry) 0)
                     (memq (nth 1 entry) used)
                     (not (memq entry selected)))
            (push entry selected)
            (setq grew t)))))
    (append (seq-filter (lambda (entry) (= (nth 3 entry) 0)) (seq-filter (lambda (entry) (memq entry selected)) all))
            (seq-filter (lambda (entry) (= (nth 3 entry) 1)) (seq-filter (lambda (entry) (memq entry selected)) all)))))

(defun agd--evaluate ()
  (dolist (entry (agd--selected))
    (pcase-let ((`(,kind ,symbol ,value ,_phase ,file ,line) entry))
      (let ((location (format "%s:%d" file line)))
        (condition-case error
            (let ((result (eval value t)))
              (pcase kind
                ('set (set-default symbol result))
                ('default (unless (default-boundp symbol) (set-default symbol result)))
                ('add (add-to-list symbol result))
                ('append (add-to-list symbol result t))
                ('push (set-default symbol (cons result (default-value symbol)))))
              (when (memq symbol agd--imported)
                (puthash symbol location agd--origins)))
          (error
           (when (memq symbol agd--imported)
             (agd--diagnostic "unsupported" (symbol-name symbol)
                              (format "%s: %s; set it in config.toml"
                                      location (error-message-string error))))))))))

(defun agd--report ()
  "Where each imported setting came from, and settings agd does not use."
  (maphash (lambda (symbol location)
             (agd--diagnostic "exact" (symbol-name symbol) location))
           agd--origins)
  (dolist (symbol '(org-agenda-files org-todo-keywords org-capture-templates
                    org-agenda-custom-commands))
    (unless (gethash symbol agd--origins)
      (agd--diagnostic "difference" (symbol-name symbol)
                       "not set in these files; Org's default applies")))
  (let ((unused (make-hash-table)))
    (dolist (entry (reverse agd--assignments))
      (let ((symbol (nth 1 entry)))
        (when (and (not (memq symbol agd--imported))
                   (not (gethash symbol unused))
                   (seq-some (lambda (prefix) (string-prefix-p prefix (symbol-name symbol)))
                             agd--unused-prefixes))
          (puthash symbol t unused)
          (agd--diagnostic "difference" (symbol-name symbol)
                           (format "%s:%d: not used by agd" (nth 4 entry) (nth 5 entry))))))))

(defun agd--string (value)
  (cond ((null value) "nil")
        ((stringp value) value)
        ((symbolp value) (symbol-name value))
        ((characterp value) (char-to-string value))
        (t (format "%S" value))))

(defun agd--strings (values)
  (vconcat (mapcar (lambda (value) (agd--string (if (consp value) (car value) value)))
                   (seq-filter (lambda (value) (not (keywordp (if (consp value) (car value) value))))
                               (or values '())))))

(defun agd--todo-sequences ()
  "The keywords of each (sequence ...) or (type ...) in org-todo-keywords.
An old-style flat list of keywords is one sequence, as Org reads it."
  (let ((sequences (if (stringp (car org-todo-keywords))
                       (list (cons 'sequence org-todo-keywords))
                     org-todo-keywords)))
    (vconcat
     (mapcar (lambda (sequence)
               (vconcat (mapcar #'agd--string (cdr sequence))))
             sequences))))

(defun agd--target-file (value)
  "A capture target file: a string, or a variable holding one, resolved
against org-directory as org-capture does."
  (let ((file (cond ((stringp value) value)
                    ((and (symbolp value) (boundp value) (stringp (symbol-value value)))
                     (symbol-value value)))))
    (and file (expand-file-name file org-directory))))

(defun agd--capture-template (entry)
  (let* ((key (nth 0 entry))
         (description (nth 1 entry))
         (kind (nth 2 entry))
         (target (nth 3 entry))
         (body (nth 4 entry))
         (enabled (eq kind 'entry))
         (diagnostics (unless (eq kind 'entry)
                        (list (format "%s templates are unsupported; only entry" kind))))
         (file (and (consp target) (agd--target-file (cadr target))))
         (value (make-hash-table :test 'equal)))
    (puthash "desc" description value)
    (puthash "datetree" :json-false value)
    (cond
     ((stringp body)
      (puthash "template" body value)
      (when (string-match-p "%[(]" body)
        (setq enabled nil diagnostics (cons "%(sexp) template forms are disabled" diagnostics))))
     ((and (consp body) (eq (car body) 'file) (agd--target-file (cadr body)))
      (puthash "template" "" value)
      (puthash "template_file" (agd--target-file (cadr body)) value))
     (t
      (puthash "template" "" value)
      (setq enabled nil diagnostics (cons "a computed template body is unsupported" diagnostics))))
    (pcase (car-safe target)
      ((guard (null file))
       (unless (eq (car-safe target) 'id)
         (setq enabled nil diagnostics (cons "a computed capture target is unsupported" diagnostics))))
      ('file (puthash "file" file value))
      ((guard (and (eq (car target) 'file+headline) (stringp (nth 2 target))))
       (puthash "file" file value)
       (puthash "heading" (nth 2 target) value))
      ('file+olp
       (puthash "file" file value)
       (puthash "outline_path" (vconcat (cddr target)) value))
      ((or 'file+datetree 'file+olp+datetree)
       (puthash "file" file value)
       (puthash "datetree" t value))
      (_ (setq enabled nil diagnostics
               (cons (format "capture target %s is unsupported" (car target)) diagnostics))))
    (when (and (eq (car-safe target) 'id) (stringp (cadr target)))
      (setq enabled (eq kind 'entry))
      (puthash "id" (cadr target) value))
    (puthash "enabled" (if enabled t :json-false) value)
    (puthash "diagnostics" (vconcat (nreverse diagnostics)) value)
    (cons key value)))

(defun agd--captures ()
  "The templates in the order written; for a repeated key the first wins, as
org-capture's menu picks it."
  (let ((seen nil) (result nil))
    (dolist (entry (or (and (boundp 'org-capture-templates) org-capture-templates) '()))
      (when (and (proper-list-p entry) (>= (length entry) 5))
        (let ((normalized (agd--capture-template entry)))
          (unless (member (car normalized) seen)
            (push (car normalized) seen)
            (puthash "key" (car normalized) (cdr normalized))
            (push (cdr normalized) result)))))
    (vconcat (nreverse result))))

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

;; org-agenda-custom-commands -> views. Only literal option values are read.
(defconst agd--view-kinds
  '((agenda . "agenda") (todo . "todo") (alltodo . "alltodo")
    (tags . "tags") (tags-todo . "tags-todo")))

(defun agd--literal (form)
  (cond ((and (consp form) (eq (car form) 'quote)) (cadr form))
        ((or (stringp form) (numberp form) (symbolp form)) form)
        (t :agd-dynamic)))

;; org-super-agenda groups -> agd groups. Each selector becomes a tags/TODO
;; match held as alternatives of terms (joined by | and +), so :and, :not
;; and a group's implicit OR of its selectors translate exactly.
(defvar agd--group-notes nil "Differences found while translating groups.")

(defun agd--and (left right)
  "Alternatives matching both LEFT and RIGHT."
  (let (result)
    (dolist (a left)
      (dolist (b right)
        (push (append a b) result)))
    (nreverse result)))

(defun agd--not (alternatives)
  "Alternatives matching none of ALTERNATIVES."
  (let ((result '(())))
    (dolist (terms alternatives)
      (setq result
            (agd--and result
                      (mapcar (lambda (term)
                                (list (if (string-prefix-p "-" term)
                                          (substring term 1)
                                        (concat "-" term))))
                              terms))))
    result))

(defun agd--match-word (value)
  (let ((word (agd--string value)))
    (unless (string-match-p "\\`[[:alnum:]_@#%]+\\'" word)
      (throw 'agd-group (format "%S cannot be written in a match" word)))
    word))

(defun agd--match-string (value)
  (let ((text (agd--string value)))
    (when (string-match-p "\"" text)
      (throw 'agd-group (format "%S cannot be written in a match" text)))
    (format "\"%s\"" text)))

(defun agd--each (values fn)
  "One alternative per value, each a single term from FN."
  (mapcar (lambda (value) (list (funcall fn value)))
          (if (listp values) values (list values))))

(defun agd--date-selector (name args)
  (pcase (if (listp args) (car args) args)
    ('t `((,(format "%s<>\"\"" name))))
    ('nil `((,(format "%s=\"\"" name))))
    ('past `((,(format "%s<\"<today>\"" name))))
    ('today `((,(format "%s>=\"<today>\"" name) ,(format "%s<\"<tomorrow>\"" name))))
    ('future `((,(format "%s>=\"<tomorrow>\"" name))))
    (_ (throw 'agd-group (format ":%s %S is not supported" (downcase name) args)))))

(defun agd--selector (selector args)
  "Alternatives for one org-super-agenda SELECTOR."
  (pcase selector
    (:todo (pcase (if (listp args) (car args) args)
             ('t '(("TODO<>\"\"")))
             ('nil '(("TODO=\"\"")))
             (_ (agd--each args (lambda (keyword) (concat "TODO=" (agd--match-string keyword)))))))
    (:tag
     (push "tags compare case-sensitively; org-super-agenda ignores case" agd--group-notes)
     (agd--each args #'agd--match-word))
    (:category (agd--each args (lambda (category) (concat "CATEGORY=" (agd--match-string category)))))
    (:priority
     (when (member (char-to-string org-priority-default)
                   (mapcar #'agd--string (if (listp args) args (list args))))
       (push (format "headings without a priority cookie count as %c; org-super-agenda leaves them out"
                     org-priority-default)
             agd--group-notes))
     (agd--each args (lambda (priority) (concat "PRIORITY=" (agd--match-string priority)))))
    (:deadline (agd--date-selector "DEADLINE" args))
    (:scheduled (agd--date-selector "SCHEDULED" args))
    (:property
     (push "properties are read from the heading only; org-super-agenda also reads ancestors"
           agd--group-notes)
     (let ((values (if (listp args) args (list args))))
       (unless (and (stringp (car values)) (or (null (cdr values)) (stringp (cadr values))))
         (throw 'agd-group (format ":property %S is not supported" args)))
       (let ((name (agd--match-word (car values))))
         (if (cdr values)
             `((,(concat name "=" (agd--match-string (cadr values)))))
           `((,(concat name "<>\"\"")))))))
    (:anything '(()))
    (:and (let ((result '(())))
            (cl-loop for (key value) on args by #'cddr
                     do (setq result (agd--and result (agd--selector key value))))
            result))
    (:not (agd--not (agd--selectors args)))
    (_ (throw 'agd-group (format "%s is not supported" selector)))))

(defun agd--selectors (plist)
  "Alternatives for a plist of selectors, which org-super-agenda ORs."
  (let (result)
    (cl-loop for (selector args) on plist by #'cddr
             do (setq result (append result (agd--selector selector args))))
    result))

(defun agd--match (alternatives)
  "ALTERNATIVES as a match; nil when one of them matches every row."
  (cond
   ((null alternatives) (throw 'agd-group "a group matches nothing"))
   ((member nil alternatives) nil)
   (t (mapconcat (lambda (terms)
                   (concat (car terms)
                           (mapconcat (lambda (term)
                                        (if (string-prefix-p "-" term) term (concat "+" term)))
                                      (cdr terms) "")))
                 alternatives "|"))))

(defun agd--take-effort (plist result)
  "PLIST without its :effort< and :effort> selectors, whose durations go
into RESULT as effort_max and effort_min (both inclusive, as in
org-super-agenda)."
  (let (rest)
    (cl-loop for (key value) on plist by #'cddr
             do (pcase key
                  (:effort< (puthash "effort_max" (agd--string (if (consp value) (car value) value)) result))
                  (:effort> (puthash "effort_min" (agd--string (if (consp value) (car value) value)) result))
                  (_ (setq rest (append rest (list key value))))))
    rest))

(defconst agd--auto-groups
  '((:auto-category . "category") (:auto-parent . "parent")
    (:auto-todo . "todo") (:auto-priority . "priority")))

(defun agd--group (group)
  "One agd group for an org-super-agenda GROUP plist."
  (let ((result (make-hash-table :test 'equal))
        (selectors nil))
    (cl-loop for (key value) on group by #'cddr
             do (cond
                 ((eq key :name) (puthash "name" (agd--string value) result))
                 ((eq key :order) (puthash "order" value result))
                 ((memq key '(:face :transformer))
                  (push (format "%s is not imported" key) agd--group-notes))
                 ((eq key :discard)
                  (puthash "discard" t result)
                  (setq selectors (append selectors value)))
                 ((assq key agd--auto-groups)
                  (puthash "by" (cdr (assq key agd--auto-groups)) result))
                 (t (setq selectors (append selectors (list key value))))))
    (if (gethash "by" result)
        (when selectors
          (throw 'agd-group "an :auto- group with other selectors is not supported"))
      ;; An effort bound holds with the match (AND): alone, or inside :and.
      (let* ((only-and (and (eq (car selectors) :and) (= (length selectors) 2)))
             (rest (agd--take-effort (if only-and (cadr selectors) selectors) result))
             (effort (or (gethash "effort_max" result) (gethash "effort_min" result))))
        (when (and effort rest (not only-and))
          (throw 'agd-group "an :effort< or :effort> ORed with other selectors is not supported"))
        (unless (and effort (null rest))
          (let ((match (agd--match (agd--selectors (if only-and (list :and rest) rest)))))
            (when match (puthash "match" match result))))))
    result))

(defun agd--groups (form)
  "(GROUPS . NOTES) for an org-super-agenda-groups option FORM, or (nil .
NOTES) when they cannot be imported."
  (let* ((agd--group-notes nil)
         (value (agd--literal form))
         (value (if (and value (symbolp value) (not (keywordp value)))
                    (if (boundp value) (symbol-value value) :agd-unbound)
                  value))
         (groups (cond
                  ((eq value :agd-dynamic) "the value is computed")
                  ((eq value :agd-unbound) (format "%s is not set in these files" form))
                  ((not (listp value)) (format "%S is not a list of groups" value))
                  (t (catch 'agd-group (vconcat (mapcar #'agd--group value)))))))
    (if (stringp groups)
        (cons nil (list (format "groups left out, rows ungrouped: %s" groups)))
      (cons groups
            (mapcar (lambda (note) (concat "groups: " note))
                    (delete-dups (reverse agd--group-notes)))))))

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
            (puthash "span" value block))
           ((and (eq symbol 'org-agenda-span) (eq value 'fortnight))
            (puthash "span" 14 block))
           ((and (eq symbol 'org-agenda-overriding-header) (stringp value))
            (puthash "header" value block))
           ((eq symbol 'org-super-agenda-groups)
            (let ((result (agd--groups (car-safe (cdr-safe option)))))
              (when (car result)
                (puthash "groups" (car result) block))
              (setq notes (append (reverse (cdr result)) notes))))
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

(defun agd--command-p (entry)
  "Whether ENTRY of org-agenda-custom-commands is a command. A (KEY .
DESCRIPTION) entry only labels a prefix key, and is skipped."
  (and (proper-list-p entry) (> (length entry) 2)))

(defun agd--commands ()
  (vconcat
   (mapcar #'agd--portable-command
           (seq-filter #'agd--command-p
                       (or (and (boundp 'org-agenda-custom-commands)
                                org-agenda-custom-commands)
                           '())))))

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

(defun agd--agenda-files ()
  "org-agenda-files as written, made absolute against org-directory."
  (cond
   ((stringp org-agenda-files)
    (agd--diagnostic "unsupported" "org-agenda-files"
                     "names a file that lists the agenda files; list them in agenda_files")
    [])
   (t (vconcat (mapcar (lambda (entry) (expand-file-name entry org-directory))
                       org-agenda-files)))))

(defun agd--verification-rows ()
  (let ((result (make-hash-table :test 'equal)))
    (when (or agd-import-verify (equal (getenv "AGD_IMPORT_VERIFY") "1"))
      (puthash "a" (with-output-to-string (org-batch-agenda-csv "a")) result)
      (dolist (entry (or (and (boundp 'org-agenda-custom-commands)
                              org-agenda-custom-commands)
                         '()))
        (when (and (agd--command-p entry)
                   (stringp (car entry))
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

(defun agd-import-read ()
  (dolist (file (split-string (or (getenv "AGD_IMPORT_FILES") "") "\n" t))
    (agd--read-file file))
  (agd--evaluate)
  (agd--report)
  (let ((agenda-files (agd--agenda-files)))
    (json-encode
     `((editor . "emacs")
       (editor_version . ,emacs-version)
       (org_directory . ,(and (gethash 'org-directory agd--origins) org-directory))
       (agenda_files . ,agenda-files)
       (todo_sequences . ,(agd--todo-sequences))
       (tag_alist . ,(agd--strings org-tag-alist))
       (persistent_tags . ,(agd--strings org-tag-persistent-alist))
       (tags_column . ,org-tags-column)
       (tag_inheritance_exclusions . ,(agd--strings org-tags-exclude-from-inheritance))
       (highest_priority . ,(char-to-string org-priority-highest))
       (lowest_priority . ,(char-to-string org-priority-lowest))
       (default_priority . ,(char-to-string org-priority-default))
       (deadline_warning_days . ,org-deadline-warning-days)
       (agenda_span . ,(agd--string org-agenda-span))
       (agenda_start_day . ,org-agenda-start-day)
       (week_start . ,(if (eql org-agenda-start-on-weekday 0) "sunday" "monday"))
       (skip_scheduled_if_done . ,(if org-agenda-skip-scheduled-if-done t :json-false))
       (skip_deadline_if_done . ,(if org-agenda-skip-deadline-if-done t :json-false))
       (skip_timestamp_if_done . ,(if org-agenda-skip-timestamp-if-done t :json-false))
       (sorting_strategy . ,(agd--sorting-strategy))
       (log_done . ,(agd--string org-log-done))
       (log_repeat . ,(agd--string org-log-repeat))
       (log_reschedule . ,(agd--string org-log-reschedule))
       (log_redeadline . ,(agd--string org-log-redeadline))
       (log_into_drawer . ,(if org-log-into-drawer t :json-false))
       (capture . ,(agd--captures))
       (commands . ,(agd--commands))
       (verification . ,(agd--verification-rows))
       (diagnostics . ,(vconcat (nreverse agd--diagnostics)))))))

(princ "AGD_JSON:")
(princ (agd-import-read))
(terpri)
